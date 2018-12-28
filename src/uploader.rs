use std::fmt;
use std::path::Path;

use failure::Error;
use rusoto_core::{request::HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_s3::{PutObjectRequest, S3Client, S3};

use config::Uploading as UploadingConfig;

pub struct Uploader {
    client: S3Client,
}

impl fmt::Debug for Uploader {
    fn fmt(&self, _formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
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

        Ok(Self { client })
    }

    pub fn upload_file(&self, file: &Path, bucket: &str) -> Result<(), Error> {
        // TODO: StreamingBody, anyone?
        let req = PutObjectRequest {
            bucket: String::from(bucket),
            ..Default::default()
        };
        self.client.put_object(req).sync()?;
        Ok(())
    }
}
