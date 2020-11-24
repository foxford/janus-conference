# stream.read

Registers the current session as reader of the stream with specified `id`.

The `id` is a string chosen by the writer during [stream.create](api.stream.create.md) method call.

Any following RTP/RTCP packets sent by the writer will be transmitted to the caller's session.
The reader may subscribe to the stream both before and after the writer starts it.

Before calling this method one should call [signal.create](api.singal.create) in order to initialize
WebRTC connection. The SDP offer sent there must be `recvonly` or `sendrecv`.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name          | Type   | Default    | Description
------------- | ------ | ---------- | -----------
body.method   | string | _required_ | Always `stream.read`.
body.id       | string | _required_ | Unique ID of the stream to read.

## Response

You should get a Janus event with specified `transaction` and following body:

Name      | Type   | Default    | Description
--------- | ------ | ---------- | -----------
status    | int    | _required_ | If status is equal to 200 then everything went well otherwise an error occurred (see [error object](./api.error.md)).
