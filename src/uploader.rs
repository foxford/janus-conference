use std::fmt;
use std::fs::File;
use std::path::Path;

use failure::Error;
use futures::Stream;
use futures_fs::FsPool;
use rusoto_core::{credential::StaticProvider, request::HttpClient, ByteStream, Region};
use rusoto_s3::{PutObjectRequest, S3Client, S3};

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
        let request_dispatcher = HttpClient::new()?;
        let credential_provider = StaticProvider::new_minimal(config.access_key, config.secret_key);
        let region = Region::Custom {
            name: config.region,
            endpoint: config.endpoint,
        };

        let client = S3Client::new_with(request_dispatcher, credential_provider, region);

        Ok(Self {
            client,
            fs_pool: FsPool::default(),
        })
    }

    pub fn upload_file(&self, file: &Path, bucket: &str, room_id: &str) -> Result<(), Error> {
        let file = File::open(file)?;
        let read = self
            .fs_pool
            .read_file(file, Default::default())
            .map(|buf| buf.to_vec());
        let streaming_body = ByteStream::new(read);

        let key = format!("{}.source.mp4", room_id);

        let req = PutObjectRequest {
            bucket: String::from(bucket),
            body: Some(streaming_body),
            key: key,
            ..Default::default()
        };
        self.client.put_object(req).sync()?;
        Ok(())
    }
}
