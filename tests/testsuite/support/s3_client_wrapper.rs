use std::env;
use std::path::Path;

use failure::Error;
use rusoto_core::Region;
use rusoto_s3::{DeleteObjectRequest, GetObjectRequest, S3Client, S3};
use s4::S4;

// A wrapper for S3 client with more concise API for readability.
pub struct S3ClientWrapper {
    client: S3Client,
}

impl S3ClientWrapper {
    pub fn new() -> Result<Self, Error> {
        let region = Region::Custom {
            name: env::var("APP_UPLOADING__REGION")?,
            endpoint: env::var("APP_UPLOADING__ENDPOINT")?,
        };

        let access_key_id = env::var("APP_UPLOADING__ACCESS_KEY_ID")?;
        let secret_access_key = env::var("APP_UPLOADING__SECRET_ACCESS_KEY")?;
        let client = s4::new_s3client_with_credentials(region, access_key_id, secret_access_key)?;
        Ok(Self { client })
    }

    pub fn get_object<P>(&self, bucket: &str, object: &str, destination: P) -> Result<(), Error>
    where
        P: AsRef<Path>,
    {
        let request = GetObjectRequest {
            bucket: bucket.to_owned(),
            key: object.to_owned(),
            ..Default::default()
        };

        self.client.download_to_file(request, destination)?;
        Ok(())
    }

    pub fn delete_object(&self, bucket: &str, object: &str) -> Result<(), Error> {
        let request = DeleteObjectRequest {
            bucket: bucket.to_owned(),
            key: object.to_owned(),
            ..Default::default()
        };

        self.client.delete_object(request).sync()?;
        Ok(())
    }
}
