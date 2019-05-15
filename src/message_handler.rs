use std::sync::{mpsc, RwLock};

use failure::{err_msg, Error};
use janus::{JanssonDecodingFlags, JanssonValue};
use rayon::{ThreadPool, ThreadPoolBuilder};

use crate::codecs::{AudioCodec, VideoCodec};
use crate::conf::Config;
use crate::messages::{
    APIError, Create, ErrorStatus, JsepKind, Read, Response, StreamOperation, StreamResponse,
    Upload,
};
use crate::recorder::Recorder;
use crate::switchboard::Switchboard;
use crate::uploader::Uploader;
use crate::{utils, ConcreteRecorder, Event, Message};

#[derive(Debug)]
pub struct MessageHandler {
    pub tx: mpsc::SyncSender<Event>,
    pub switchboard: RwLock<Switchboard>,
    pub config: Config,
    pub uploader: Uploader,
    pub thread_pool: ThreadPool,
}

impl MessageHandler {
    pub fn new(config: Config, tx: mpsc::SyncSender<Event>) -> Result<Self, Error> {
        let switchboard = RwLock::new(Switchboard::new());

        let uploader = Uploader::new(config.uploading.clone())
            .map_err(|err| format_err!("Failed to init uploader: {}", err))?;

        let thread_pool = ThreadPoolBuilder::new()
            .build()
            .map_err(|err| format_err!("Failed to initialize thread pool: {}", err))?;

        Ok(Self {
            tx,
            switchboard,
            config,
            uploader,
            thread_pool,
        })
    }

    pub fn handle(&self, msg: &Message) {
        let result = match msg.operation.clone() {
            Some(StreamOperation::Create { ref id }) => self.handle_create(msg, id),
            Some(StreamOperation::Read { ref id }) => self.handle_read(msg, id),
            Some(StreamOperation::Upload {
                ref id,
                ref bucket,
                ref object,
            }) => self.handle_upload(msg, id, bucket, object),
            None => {
                let err = err_msg("Missing operation");
                Err(APIError::new(
                    ErrorStatus::INTERNAL_SERVER_ERROR,
                    err,
                    &None,
                ))
            }
        };

        if let Err(err) = result {
            self.respond(msg, Err(err), None);
        }
    }

    pub fn respond(
        &self,
        msg: &Message,
        result: Result<StreamResponse, APIError>,
        jsep: Option<JanssonValue>,
    ) {
        let (response, jsep) = match result {
            Ok(response) => match Self::build_ok_response(response, msg.operation.clone()) {
                Ok(response) => (response, jsep),
                Err(err) => (Self::build_error_response(err), None),
            },
            Err(err) => {
                janus_err!("Error processing message: {}", err);
                (Self::build_error_response(err), None)
            }
        };

        let response = Event::Response {
            msg: msg.to_owned(),
            response: Some(response),
            jsep,
        };

        self.tx.send(response).ok();
    }

    // TODO: move to StreamResponse.into_raw_response().
    fn build_ok_response(
        response: StreamResponse,
        operation: Option<StreamOperation>,
    ) -> Result<JanssonValue, APIError> {
        let response =
            serde_json::to_value(Response::new(Some(response), None)).map_err(|err| {
                APIError::new(
                    ErrorStatus::INTERNAL_SERVER_ERROR,
                    Error::from(err),
                    &operation,
                )
            })?;

        let response = utils::serde_to_jansson(&response)
            .map_err(|err| APIError::new(ErrorStatus::INTERNAL_SERVER_ERROR, err, &operation))?;

        Ok(response)
    }

    fn build_error_response(err: APIError) -> JanssonValue {
        serde_json::to_value(Response::new(None, Some(err)))
            .map_err(|_| err_msg("Error dumping response to JSON"))
            .and_then(|response| utils::serde_to_jansson(&response))
            .unwrap_or_else(|err| {
                let err = format!("Error serializing other error: {}", &err);

                JanssonValue::from_str(&err, JanssonDecodingFlags::empty())
                    .unwrap_or_else(|_| Self::json_serialization_fallback_error())
            })
    }

    // TODO: make it `const fn` in future Rust versions. Now it fails with:
    // `error: trait bounds other than `Sized` on const fn parameters are unstable`
    fn json_serialization_fallback_error() -> JanssonValue {
        // `unwrap` is ok here because we're converting a constant string.
        JanssonValue::from_str("JSON serialization error", JanssonDecodingFlags::empty()).unwrap()
    }

    fn handle_create(&self, msg: &Message, id: &str) -> Result<(), APIError> {
        let jsep = Self::build_jsep(&msg)?;

        let mut switchboard = self.switchboard.write().map_err(|_| {
            APIError::new(
                ErrorStatus::INTERNAL_SERVER_ERROR,
                err_msg("Failed to acquire switchboard write lock"),
                &msg.operation,
            )
        })?;

        switchboard.create_stream(id.to_owned(), msg.session.clone());

        if self.config.recordings.enabled {
            let mut recorder = ConcreteRecorder::new(&self.config.recordings, &id);

            recorder.start_recording().map_err(|err| {
                APIError::new(
                    ErrorStatus::INTERNAL_SERVER_ERROR,
                    Error::from(err),
                    &msg.operation,
                )
            })?;

            switchboard.attach_recorder(msg.session.clone(), recorder);
        }

        let response = StreamResponse::CreateStreamResponse(Create::new());
        self.respond(msg, Ok(response), jsep);
        Ok(())
    }

    fn handle_read(&self, msg: &Message, id: &str) -> Result<(), APIError> {
        let jsep = Self::build_jsep(&msg)?;

        let mut switchboard = self.switchboard.write().map_err(|_| {
            APIError::new(
                ErrorStatus::INTERNAL_SERVER_ERROR,
                err_msg("Failed to acquire switchboard write lock"),
                &msg.operation,
            )
        })?;

        switchboard
            .join_stream(&String::from(id), msg.session.clone())
            .map_err(|err| {
                APIError::new(ErrorStatus::NOT_FOUND, Error::from(err), &msg.operation)
            })?;

        let response = StreamResponse::ReadStreamResponse(Read::new());
        self.respond(msg, Ok(response), jsep);
        Ok(())
    }

    fn handle_upload(
        &self,
        msg: &Message,
        id: &str,
        bucket: &str,
        object: &str,
    ) -> Result<(), APIError> {
        let switchboard = self.switchboard.read().map_err(|_| {
            APIError::new(
                ErrorStatus::INTERNAL_SERVER_ERROR,
                err_msg("Failed to acquire switchboard read lock"),
                &msg.operation,
            )
        })?;

        // Stopping active recording if any.
        if let Some(publisher) = switchboard.publisher_by_stream(&String::from(id)) {
            if let Some(recorder) = switchboard.recorder_for(publisher) {
                recorder.stop_recording().map_err(|err| {
                    APIError::new(
                        ErrorStatus::INTERNAL_SERVER_ERROR,
                        Error::from(err),
                        &msg.operation,
                    )
                })?;
            }
        }

        let mut recorder = ConcreteRecorder::new(&self.config.recordings, &id);

        self.thread_pool
            .install(move || {
                let start_stop_timestamps = match recorder.finish_record() {
                    Ok(start_stop_timestamps) => start_stop_timestamps,
                    Err(err) => {
                        self.respond(
                            msg,
                            Err(APIError::new(
                                ErrorStatus::INTERNAL_SERVER_ERROR,
                                Error::from(err),
                                &msg.operation,
                            )),
                            None,
                        );

                        return Err(());
                    }
                };

                let path = recorder.get_full_record_path();

                match self.uploader.upload_file(&path, &bucket, &object) {
                    Ok(_) => {}
                    Err(err) => {
                        self.respond(
                            msg,
                            Err(APIError::new(
                                ErrorStatus::INTERNAL_SERVER_ERROR,
                                Error::from(err),
                                &msg.operation,
                            )),
                            None,
                        );

                        return Err(());
                    }
                };

                let upload = Upload::new(start_stop_timestamps);
                let response = StreamResponse::UploadStreamResponse(upload);
                self.respond(msg, Ok(response), None);
                Ok(())
            })
            .map_err(|()| {
                APIError::new(
                    ErrorStatus::INTERNAL_SERVER_ERROR,
                    err_msg("Error queueing upload job"),
                    &msg.operation,
                )
            })?;

        Ok(())
    }

    fn build_jsep(msg: &Message) -> Result<Option<JanssonValue>, APIError> {
        let jsep_offer_parse_result = msg
            .jsep
            .clone()
            .ok_or_else(|| {
                APIError::new(
                    ErrorStatus::BAD_REQUEST,
                    err_msg("JSEP is empty"),
                    &msg.operation,
                )
            })
            .and_then(|ref jsep| {
                JanssonValue::from_str(jsep, JanssonDecodingFlags::empty()).map_err(|err| {
                    APIError::new(
                        ErrorStatus::INTERNAL_SERVER_ERROR,
                        format_err!("Failed to deserialize JSEP: {}", err),
                        &msg.operation,
                    )
                })
            })
            .map(|ref jsep| utils::jansson_to_serde::<JsepKind>(jsep))?;

        let offer = jsep_offer_parse_result
            .map_err(|err| APIError::new(ErrorStatus::BAD_REQUEST, err, &msg.operation))?;

        let video_codec = <ConcreteRecorder as Recorder>::VideoCodec::SDP_VIDEO_CODEC;
        let audio_codec = <ConcreteRecorder as Recorder>::AudioCodec::SDP_AUDIO_CODEC;
        let answer = offer.negotatiate(video_codec, audio_codec);

        let jsep = serde_json::to_value(answer).map_err(|err| {
            APIError::new(
                ErrorStatus::INTERNAL_SERVER_ERROR,
                Error::from(err),
                &msg.operation,
            )
        })?;

        let jsep = utils::serde_to_jansson(&jsep).map_err(|err| {
            APIError::new(ErrorStatus::INTERNAL_SERVER_ERROR, err, &msg.operation)
        })?;

        msg.session.set_offer(offer).map_err(|err| {
            APIError::new(ErrorStatus::INTERNAL_SERVER_ERROR, err, &msg.operation)
        })?;

        Ok(Some(jsep))
    }
}
