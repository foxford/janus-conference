use std::fmt;
use std::path::Path;
use std::fs::File;

use failure::Error;
use rusoto_core::{request::HttpClient, Region, ByteStream, credential::StaticProvider};
use rusoto_s3::{PutObjectRequest, S3Client, S3};
use s4;
use s4::S4;
use futures_fs::FsPool;
use futures::Stream;
use fallible_iterator::FallibleIterator;

use config::Uploading as UploadingConfig;

pub struct Uploader {
    client: S3Client,
    fs_pool: FsPool,
}

impl fmt::Debug for Uploader {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(formatter, "<<Uploader>>")?;

        Ok(())
    }
}

impl Uploader {
    pub fn new(config: UploadingConfig) -> Result<Self, Error> {
        env_logger::init();
        janus_info!("{:?}", config);

        // let request_dispatcher = HttpClient::new()?;
        // let credential_provider = StaticProvider::new_minimal(config.access_key, config.secret_key);
        let region = Region::Custom {
            name: config.region,
            endpoint: config.endpoint,
        };

        let client = s4::new_s3client_with_credentials(region, config.access_key, config.secret_key)?;

        Ok(Self {
            client,
            fs_pool: FsPool::default(),
        })
    }

    pub fn upload_file(&self, file: &Path, bucket: &str) -> Result<(), Error> {
        // let file = File::open(file)?;
        // let read = self.fs_pool.read_file(file, Default::default()).map(|buf| buf.to_vec());
        // let streaming_body = ByteStream::new(read);

        let req = PutObjectRequest {
            bucket: String::from(bucket),
            // body: Some(streaming_body),
            // TODO: filename as ${FILENAME}.source.mp4
            key: String::from("demo-conference-room.source.mp4"),
            ..Default::default()
        };
        self.client.upload_from_file(file, req)?;
        Ok(())
    }
}
