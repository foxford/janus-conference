#!/bin/bash -e

function REPORT_ERROR() { >&2 echo ${@}; }

SCRIPT_ABS_PATH=$(realpath $0)
SCRIPT_ABS_DIR=$(dirname ${SCRIPT_ABS_PATH})

###############################################################################

# Arguments.
RTC_ID=$1
BACKEND=$2
BUCKET=$3

if [[ ! ${RTC_ID} ]]; then $(REPORT_ERROR "RTC_ID isn't specified"); exit 1; fi
if [[ ! ${BACKEND} ]]; then $(REPORT_ERROR "BACKEND isn't specified"); exit 1; fi
if [[ ! ${BUCKET} ]]; then $(REPORT_ERROR "BUCKET isn't specified"); exit 1; fi

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
JANUS_PP_REC="${SCRIPT_ABS_DIR}/janus-pp-rec -d 4"
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

# Remove artifacts from possible previous run to avoid concat duplication.
rm -f dumps.txt

for FILE in *.mjr; do
  DUMP_FILE="s3://${BUCKET}/${RTC_ID}_dump/${FILE}"
  ${AWS} s3 cp ${FILE} ${DUMP_FILE} \
    --only-show-errors \
    --cache-control 'no-cache'
  echo ${DUMP_FILE} >> dumps.txt
done
