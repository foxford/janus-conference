# stream.read

Read a real-time connection in order to initialize signaling phase and receive media.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name         | Type   | Default    | Description
------------ | ------ | ---------- | -----------
body.method  | string | _required_ | Always `stream.read`.
body.id      | string | _required_ | Unique ID of the stream you want to start. This string is used to group publishers and subscribers. **It's up to you to generate these IDs and ensure their consistency.**
jsep.type    | string | _required_ | Always `offer`.
jsep.sdp     | string | _required_ | An SDP offer

## Response

You should get a Janus event with specified `transaction` and following body:

Name      | Type   | Default    | Description
--------- | ------ | ---------- | -----------
status    | int    | _required_ | If status is equal to 200 then everything went well otherwise an error occurred (see [error object](./api.error.md)).
jsep.type | string | _required_ | Always `answer`
jsep.sdp  | string | _required_ | An SDP answer
