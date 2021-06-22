# reader_config.update

Allows muting and unmuting video or audio for specific readers in bulk.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name          | Type     | Default    | Description
------------- | -------- | ---------- | ---------------------------
body.configs  | [object] | _required_ | An array of reader config objects.

Reader config object:

Name          | Type   | Default    | Description
------------- | ------ | ---------- | ----------------------------------------------
reader_id     | String | _required_ | Agent ID of a reader to apply the config for.
stream_id     | string | _required_ | ID of a stream which the reader is [reading](apu.stream.read.md).
receive_video | bool   | _required_ | Whether to relay video RTP packets from the stream publisher to the reader.
receive_audio | bool   | _required_ | Whether to audio video RTP packets from the stream publisher to the reader.

## Response

You should get a Janus event with specified `transaction` and following body:

Name      | Type   | Default    | Description
--------- | ------ | ---------- | -----------
status    | int    | _required_ | If status is equal to 200 then everything went well otherwise an error occurred (see [error object](./api.error.md)).
