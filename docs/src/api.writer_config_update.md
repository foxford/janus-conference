# writer_config.update

Allows controlling publisher media stream.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name          | Type     | Default    | Description
------------- | -------- | ---------- | ---------------------------
body.configs  | [object] | _required_ | An array of writer config objects.

Reader config object:

Name       | Type   | Default     | Description
---------- | ------ | ----------- | ----------------------------------------------
stream_id  | string | _required_  | ID of a stream which the writer is [writing](apu.stream.create.md) to.
send_video | bool   | _required_  | Whether to relay or drop video RTP packets sent by the writer.
send_audio | bool   | _required_  | Whether to relay or drop audio RTP packets sent by the writer.
video_remb | int    | from config | Maximum video bitrate allowed for the publisher.
audio_remb | int    | from config | Maximum audio bitrate allowed for the publisher.

## Response

You should get a Janus event with specified `transaction` and following body:

Name      | Type   | Default    | Description
--------- | ------ | ---------- | -----------
status    | int    | _required_ | If status is equal to 200 then everything went well otherwise an error occurred (see [error object](./api.error.md)).
