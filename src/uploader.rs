use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::result::Result as StdResult;

use anyhow::{bail, format_err, Context, Result};
use rusoto_core::request::HttpClient;
use rusoto_credential::StaticProvider;
use rusoto_s3::{
    AbortMultipartUploadRequest, CompleteMultipartUploadRequest, CompletedMultipartUpload,
    CompletedPart, CreateMultipartUploadRequest, S3Client, UploadPartRequest, S3,
};
use rusoto_signature::Region;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    pub region: String,
    pub endpoint: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

pub struct Uploader {
    client: S3Client,
}

impl fmt::Debug for Uploader {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> StdResult<(), fmt::Error> {
        write!(formatter, "<<Uploader>>")?;
        Ok(())
    }
}

const PART_SIZE: usize = 1024 * 1024 * 100;

impl Uploader {
    pub fn build(config: Config) -> Result<Self> {
        let region = Region::Custom {
            name: config.region,
            endpoint: config.endpoint,
        };

        let client = S3Client::new_with(
            HttpClient::new()?,
            StaticProvider::new_minimal(config.access_key_id, config.secret_access_key),
            region,
        );

        Ok(Self { client })
    }

    pub fn upload_file(&self, path: &Path, bucket: &str, object: &str) -> Result<()> {
        let mut file = File::open(&path).context("Failed to open source file for upload")?;

        let create_req = CreateMultipartUploadRequest {
            bucket: bucket.to_owned(),
            key: object.to_owned(),
            ..Default::default()
        };

        let upload_id = self
            .client
            .create_multipart_upload(create_req)
            .sync()
            .context("S3 multipart upload creation error")?
            .upload_id
            .ok_or_else(|| format_err!("S3 multipart creation response missing upload id"))?;

        match self.upload_parts(&mut file, bucket, object, &upload_id) {
            Ok(parts) => {
                let complete_req = CompleteMultipartUploadRequest {
                    bucket: bucket.to_owned(),
                    key: object.to_owned(),
                    upload_id,
                    multipart_upload: Some(CompletedMultipartUpload { parts: Some(parts) }),
                    ..Default::default()
                };

                self.client
                    .complete_multipart_upload(complete_req)
                    .sync()
                    .context("Failed to complete S3 uploading")?;

                Ok(())
            }
            Err(err) => {
                let abort_req = AbortMultipartUploadRequest {
                    bucket: bucket.to_owned(),
                    key: object.to_owned(),
                    upload_id,
                    ..Default::default()
                };

                if let Err(err) = self.client.abort_multipart_upload(abort_req).sync() {
                    janus_err!("Failed to abort S3 upload: {:?}", err);
                }

                bail!("S3 upload failed: {}", err);
            }
        }
    }

    fn upload_parts(
        &self,
        file: &mut File,
        bucket: &str,
        object: &str,
        upload_id: &str,
    ) -> Result<Vec<CompletedPart>> {
        let mut parts = Vec::new();

        for part_number in 1.. {
            let mut buffer = vec![0; PART_SIZE];

            let size = file
                .read(&mut buffer[..])
                .context("Error reading source file for upload")?;

            if size == 0 {
                break;
            }

            buffer.truncate(size);

            let upload_req = UploadPartRequest {
                bucket: bucket.to_owned(),
                key: object.to_owned(),
                upload_id: upload_id.to_owned(),
                part_number,
                body: Some(buffer.into()),
                ..Default::default()
            };

            let part = self.client.upload_part(upload_req).sync()?;

            parts.push(CompletedPart {
                part_number: Some(part_number),
                e_tag: part.e_tag,
            });
        }

        Ok(parts)
    }
}
