#!/bin/bash -e

function REPORT_ERROR() { >&2 echo ${@}; }

SCRIPT_ABS_PATH=$(realpath $0)
SCRIPT_ABS_DIR=$(dirname ${SCRIPT_ABS_PATH})

###############################################################################

# Arguments.
RTC_ID=$1
BACKEND=$2
BUCKET=$3
OBJECT=$4

if [[ ! ${RTC_ID} ]]; then $(REPORT_ERROR "RTC_ID isn't specified"); exit 1; fi
if [[ ! ${BACKEND} ]]; then $(REPORT_ERROR "BACKEND isn't specified"); exit 1; fi
if [[ ! ${BUCKET} ]]; then $(REPORT_ERROR "BUCKET isn't specified"); exit 1; fi
if [[ ! ${OBJECT} ]]; then $(REPORT_ERROR "OBJECT isn't specified"); exit 1; fi

###############################################################################

# Environment.
APP_UPLOADING_ACCESS_KEY_ID_NAME="APP_UPLOADING_${BACKEND^^}_ACCESS_KEY_ID"
APP_UPLOADING_SECRET_ACCESS_KEY_NAME="APP_UPLOADING_${BACKEND^^}_SECRET_ACCESS_KEY"
APP_UPLOADING_ENDPOINT_NAME="APP_UPLOADING_${BACKEND^^}_ENDPOINT"
APP_UPLOADING_REGION_NAME="APP_UPLOADING_${BACKEND^^}_REGION"

if [[ ! ${!APP_UPLOADING_ACCESS_KEY_ID_NAME} ]]; then $(REPORT_ERROR "${APP_UPLOADING_ACCESS_KEY_ID_NAME} isn't specified"); exit 1; fi
if [[ ! ${!APP_UPLOADING_SECRET_ACCESS_KEY_NAME} ]]; then $(REPORT_ERROR "${APP_UPLOADING_SECRET_ACCESS_KEY_NAME} isn't specified"); exit 1; fi
if [[ ! ${!APP_UPLOADING_ENDPOINT_NAME} ]]; then $(REPORT_ERROR "${APP_UPLOADING_ENDPOINT_NAME} isn't specified"); exit 1; fi
if [[ ! ${!APP_UPLOADING_REGION_NAME} ]]; then $(REPORT_ERROR "${APP_UPLOADING_REGION_NAME} isn't specified"); exit 1; fi

export AWS_ACCESS_KEY_ID=${!APP_UPLOADING_ACCESS_KEY_ID_NAME}
export AWS_SECRET_ACCESS_KEY=${!APP_UPLOADING_SECRET_ACCESS_KEY_NAME}
export AWS_ENDPOINT=${!APP_UPLOADING_ENDPOINT_NAME}
export AWS_REGION=${!APP_UPLOADING_REGION_NAME}

RECORDINGS_DIR=${RECORDINGS_DIR:-/recordings}
JANUS_PP_REC="${SCRIPT_ABS_DIR}/janus-pp-rec -d 3"
FFMPEG=${FFMPEG:-'ffmpeg -hide_banner -loglevel warning -abort_on empty_output'}
AWS=${AWS:-"aws --endpoint-url=${AWS_ENDPOINT} --region=${AWS_REGION}"}

###############################################################################

# Working directory.
cd ${RECORDINGS_DIR}/${RTC_ID}

# Try to acquire lock.
if { set -C; 2>/dev/null > vacuum_${RTC_ID}.lock; }; then
  trap "rm -f vacuum_${RTC_ID}.lock" EXIT
else
  echo "Lock file exists, exiting"
  exit 251
fi

# Upload source .mjr dumps.
for FILE in *.mjr; do
  ${AWS} s3 cp ${FILE} s3://${BUCKET}/${RTC_ID}_dump/${FILE} \
    --only-show-errors \
    --cache-control 'no-cache'
done

# Remove artifacts from possible previous run to avoid concat duplication.
rm -f video_sources.txt audio_sources.txt segments.csv

# Convert video .mjr dumps into .webm files.
for FILE in *.video.mjr; do
  OUTPUT_FILE="${FILE%.*}.webm"
  ! ${JANUS_PP_REC} ${FILE} ${OUTPUT_FILE}

  if [[ -f ${OUTPUT_FILE} ]]; then
    echo "file '${OUTPUT_FILE}'" >> video_sources.txt
  else
    >&2 echo "[ERROR] ${OUTPUT_FILE} not created; skipping segment"
  fi
done

# Get video segments durations and write to segments.csv file.
for FILE in *.video.webm; do
  DURATION=$(ffprobe -i ${FILE} -show_entries format=duration -v quiet -of csv="p=0")

  if [[ "${DURATION}" != "N/A" ]]; then
    echo "${FILE%%.*},${DURATION}" >> segments.csv
  fi
done

# Concat video segments into a single .webm file.
${FFMPEG} -f concat -i video_sources.txt -c copy -y concat.webm

# Convert audio .mjr dumps into .opus files.
for FILE in *.audio.mjr; do
  OUTPUT_FILE="${FILE%.*}.opus"
  ! ${JANUS_PP_REC} ${FILE} ${OUTPUT_FILE}

  if [[ -f ${OUTPUT_FILE} ]]; then
    echo "file '${OUTPUT_FILE}'" >> audio_sources.txt
  else
    >&2 echo "[ERROR] ${OUTPUT_FILE} not created; skipping segment"
  fi
done

# Concat audio segments into a single .opus file.
${FFMPEG} -f concat -i audio_sources.txt -c copy -y concat.opus

# Mux video & audio into a single .webm file.
${FFMPEG} -i concat.webm -i concat.opus -c copy -y full.webm

# Upload record.
${AWS} s3 cp full.webm s3://${BUCKET}/${OBJECT} \
  --only-show-errors \
  --cache-control 'no-cache' \
  --content-type 'video/webm'
