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

# Try to acquire lock
if { set -C; 2>/dev/null > vacuum_${RTC_ID}.lock; }; then
  trap "rm -f vacuum_${RTC_ID}.lock" EXIT
else
  echo "Lock file exists, exiting"
  exit 251
fi

for FILE in *.mjr; do
${AWS} s3 cp ${FILE} s3://${BUCKET}/${RTC_ID}_dump/${FILE} \
  --only-show-errors \
  --cache-control 'no-cache'
done

# Remove artifacts from possible previous run to avoid concat duplication.
rm -f sources.txt segments.csv

# Mux corresponding video and audio .mjrs and write video len to segments.csv
for VIDEO_FILE in *.video.mjr; do
  PREFIX=${VIDEO_FILE%%.*}
  AUDIO_FILE="${PREFIX}.audio.mjr"
  VIDEO_OUTPUT_FILE="${PREFIX}.video.webm"
  AUDIO_OUTPUT_FILE="${PREFIX}.audio.opus"
  OUTPUT_FILE="${PREFIX}.final.webm"

  ${JANUS_PP_REC} ${VIDEO_FILE} ${VIDEO_OUTPUT_FILE}
  ${JANUS_PP_REC} ${AUDIO_FILE} ${AUDIO_OUTPUT_FILE}

  # Extract first audio pkt write timestamp (in micros) from dump
  A_STARTED_AT=$(janus-pp-rec -H ${AUDIO_FILE}  | grep Written | awk -F ': ' '{print $2}')
  # Same but for video
  V_STARTED_AT=$(janus-pp-rec -H ${VIDEO_FILE}  | grep Written | awk -F ': ' '{print $2}')
  # Get abs diff
  DIFF=$(($A_STARTED_AT - $V_STARTED_AT))
  DIFF=${DIFF#-}
  # Convert micros to secs
  DIFF=$(awk "BEGIN { x = $DIFF/1000000; printf(\"%.2f\n\", x) }" | sed s/,/./)

  # If first audio pkt was written earlier than video pkt
  # we delay _video_ stream for $DIFF secs
  # because we started receiving audio pkts $DIFF secs earlier than video
  # we should probably cut off this $DIFF secs of "only audio" part in transcoding
  if [ $A_STARTED_AT -lt $V_STARTED_AT ]; then
    FFMPEG_INPUTS="-i ${AUDIO_OUTPUT_FILE} -itsoffset ${DIFF} -i ${VIDEO_OUTPUT_FILE}"
  elif [ $A_STARTED_AT -gt $V_STARTED_AT ]; then
    FFMPEG_INPUTS="-i ${VIDEO_OUTPUT_FILE} -itsoffset ${DIFF} -i ${AUDIO_OUTPUT_FILE}"
  else
    FFMPEG_INPUTS="-i ${VIDEO_OUTPUT_FILE} -i ${AUDIO_OUTPUT_FILE}"
  fi

  ${FFMPEG} ${FFMPEG_INPUTS} -c copy ${OUTPUT_FILE}

  if [[ -f ${OUTPUT_FILE} ]]; then
    echo "file '${OUTPUT_FILE}'" >> sources.txt

    DURATION=$(ffprobe -i ${OUTPUT_FILE} -show_entries format=duration -v quiet -of csv="p=0")

    if [[ "${DURATION}" != "N/A" ]]; then
      echo "${PREFIX},${DURATION}" >> segments.csv
    fi
  else
    >&2 echo "[ERROR] ${OUTPUT_FILE} not created; skipping segment"
  fi
done

# Concat av segments into a single .webm file.
${FFMPEG} -f concat -i sources.txt -c copy -y full.webm

# Upload record.
${AWS} s3 cp full.webm s3://${BUCKET}/${OBJECT} \
  --only-show-errors \
  --cache-control 'no-cache' \
  --content-type 'video/webm'
