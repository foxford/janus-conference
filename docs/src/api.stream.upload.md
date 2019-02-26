# stream.upload

Upload a stream record to S3 storage.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name        | Type   | Default    | Description
----------- | ------ | ---------- | -----------
body.method | String | _required_ | Always `stream.upload`.
body.id     | String | _required_ | Unique ID of the stream you want to start. This string is used to group publishers and subscribers. **It's up to you to generate these IDs and ensure their consistency.**
body.bucket | String | _required_ | Destination S3 bucket.
body.object | String | _required_ | Destination S3 object name.

## Response

You should get a Janus event with specified `transaction` and following body:

Name      | Type                   | Default    | Description
--------- | ---------------------- | ---------- | -----------
status    | Int                    | _required_ | If status is equal to 200 then everything went well otherwise an error occurred (see [error object](./api.error.md)).
time      | Array of Arrays of Int | []         | An array of start/stop recording timestamps.
