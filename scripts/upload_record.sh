#!/bin/bash -e

function REPORT_ERROR() { >&2 echo ${@}; }

SCRIPT_ABS_PATH=$(realpath $0)
SCRIPT_ABS_DIR=$(dirname ${SCRIPT_ABS_PATH})

###############################################################################

# Arguments.
RTC_ID=$1
BUCKET=$2
OBJECT=$3

if [[ ! ${RTC_ID} ]]; then $(REPORT_ERROR "RTC_ID isn't specified"); exit 1; fi
if [[ ! ${BUCKET} ]]; then $(REPORT_ERROR "BUCKET isn't specified"); exit 1; fi
if [[ ! ${OBJECT} ]]; then $(REPORT_ERROR "OBJECT isn't specified"); exit 1; fi

###############################################################################

# Environment.
if [[ ! ${APP_UPLOADING__ACCESS_KEY_ID} ]]; then $(REPORT_ERROR "APP_UPLOADING__ACCESS_KEY_ID isn't specified"); exit 1; fi
if [[ ! ${APP_UPLOADING__SECRET_ACCESS_KEY} ]]; then $(REPORT_ERROR "APP_UPLOADING__SECRET_ACCESS_KEY isn't specified"); exit 1; fi
if [[ ! ${APP_UPLOADING__ENDPOINT} ]]; then $(REPORT_ERROR "APP_UPLOADING__ENDPOINT isn't specified"); exit 1; fi
if [[ ! ${APP_UPLOADING__REGION} ]]; then $(REPORT_ERROR "APP_UPLOADING__REGION isn't specified"); exit 1; fi

export AWS_ACCESS_KEY_ID=${APP_UPLOADING__ACCESS_KEY_ID}
export AWS_SECRET_ACCESS_KEY=${APP_UPLOADING__SECRET_ACCESS_KEY}
export AWS_ENDPOINT=${APP_UPLOADING__ENDPOINT}
export AWS_REGION=${APP_UPLOADING__REGION}

RECORDINGS_DIR=${RECORDINGS_DIR:-/recordings}

###############################################################################

# Working directory.
cd ${RECORDINGS_DIR}/${RTC_ID}

# Remove artifacts from possible previous run to avoid concat duplication.
rm -f video_sources.txt audio_sources.txt segments.csv

# Convert video .mjr dumps into .webm files.
for FILE in *.video.mjr; do
  OUTPUT_FILE="${FILE%.*}.webm"
  ${SCRIPT_ABS_DIR}/janus-pp-rec ${FILE} ${OUTPUT_FILE}

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
ffmpeg -f concat -i video_sources.txt -c copy -y concat.webm

# Convert audio .mjr dumps into .opus files.
for FILE in *.audio.mjr; do
  OUTPUT_FILE="${FILE%.*}.opus"
  ${SCRIPT_ABS_DIR}/janus-pp-rec ${FILE} ${OUTPUT_FILE}

  if [[ -f ${OUTPUT_FILE} ]]; then
    echo "file '${OUTPUT_FILE}'" >> audio_sources.txt
  else
    >&2 echo "[ERROR] ${OUTPUT_FILE} not created; skipping segment"
  fi
done

# Concat audio segments into a single .opus file.
ffmpeg -f concat -i audio_sources.txt -c copy -y concat.opus

# Mux video & audio into a single .webm file.
ffmpeg -i concat.webm -i concat.opus -c copy -y full.webm

# Upload record.
aws --endpoint-url=${AWS_ENDPOINT} --region=${AWS_REGION} s3 cp full.webm s3://${BUCKET}/${OBJECT}
