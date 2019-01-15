use std::fmt;
use std::fs::File;
use std::path::Path;
use std::io::{BufRead, BufReader};

use failure::Error;
use futures::Stream;
use futures_fs::FsPool;
use rusoto_core::{credential::StaticProvider, request::HttpClient, ByteStream, Region};
use rusoto_s3::{PutObjectRequest, S3Client, S3, CreateMultipartUploadRequest, CompletedMultipartUpload, CompletedPart, UploadPartRequest, CompleteMultipartUploadRequest};
use s4::S4;

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
        // let file = File::open(file)?;
        // let read = self
        //     .fs_pool
        //     .read_file(file, Default::default())
        //     .map(|buf| buf.to_vec());
        // let streaming_body = ByteStream::new(read);

        let key = format!("{}.source.mp4", room_id);
        let bucket = String::from(bucket);

        // let upload_request = CreateMultipartUploadRequest {
        //     bucket: bucket.clone(),
        //     key: key.clone(),
        //     ..Default::default()
        // };

        // let multipart = self.client.create_multipart_upload(upload_request).sync()?;
        // let upload_id = multipart.upload_id.unwrap();

        // let mut parts: Vec<CompletedPart> = Vec::new();

        // const CAP: usize = 1024 * 1024;
        // let file = File::open(file)?;
        // let mut reader = BufReader::with_capacity(CAP, file);
        // let mut part_number = 1;

        // loop {
        //     let length = {
        //         let buffer = reader.fill_buf()?;

        //         let req = UploadPartRequest {
        //             part_number,
        //             upload_id: upload_id.clone(),
        //             key: key.clone(),
        //             bucket: bucket.clone(),
        //             body: Some(ByteStream::from(buffer.to_vec())),
        //             ..Default::default()
        //         };
        //         let part = self.client.upload_part(req).sync()?;

        //         parts.push(CompletedPart {
        //             e_tag: part.e_tag,
        //             part_number: Some(part_number)
        //         });

        //         buffer.len()
        //     };
        //     if length == 0 { break; }

        //     reader.consume(length);
        //     part_number += 1;
        // }

        // self.client.complete_multipart_upload(CompleteMultipartUploadRequest {
        //     bucket: bucket.clone(),
        //     key: key.clone(),
        //     multipart_upload: Some(CompletedMultipartUpload {parts: Some(parts)}),
        //     upload_id: upload_id.clone(),
        //     ..Default::default()
        // }).sync()?;
        let req = PutObjectRequest {
            bucket: String::from(bucket),
            // body: Some(streaming_body),
            key: key,
            ..Default::default()
        };
        self.client.upload_from_file_multipart(file, &req, 1024 * 1024 * 100)?;
        Ok(())
    }
}
