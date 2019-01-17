# stream.read

Read a real-time connection in order to initialize signaling phase and receive media.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name         | Type   | Default    | Description
------------ | ------ | ---------- | -----------
body.method  | String | _required_ | Always `stream.read`
body.id      | String | _required_ | Unique ID of the stream you want to start. This string is used to group publishers and subscribers. **It's up to you to generate these IDs and ensure their consistency.**

## Response

You should get a Janus event with specified `transaction` and following body:

Name    | Type   | Default    | Description
------- | ------ | ---------- | -----------
success                       | Bool   | _required_ | Whether operation succeeded or not. If it's false then `error` object is also returned.
error.detail                  | String | _required_ | Human-readable description of failure.
error.kind                    | String | _required_ | Whether `Internal`, `BadRequest`, `NonExistentStream`.
error.kind.BadRequest.reason  | String | _required_ | Why exactly `BadRequest` happened.
error.kind.NonExistentStream.id | String | _required_ | Id of non-existent stream.

Also, you'll get a `jsep` offer for which you should generate answer and send back with empty `body`.
