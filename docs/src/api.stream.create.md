# stream.create

Registers the current session as writer of the stream with specified `id`.

The `id` is an arbitrary string being chosen by the method caller to identify the stream.

Any following RTP/RTCP packets sent by the caller's session will be transmitted to readers who
have subscribed to the same stream `id` using [stream.read](api.stream.read.md) method.
Readers may subscribe to the stream both before or after the writer calls this method.

The previous writer gets unregistered if present.

Before calling this method one should call [signal.create](api.signal.create.md) in order
to initialize WebRTC connection. The SDP offer sent there must be `sendonly` or `sendrecv`.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name          | Type   | Default    | Description
------------- | ------ | ---------- | -----------
body.method   | string | _required_ | Always `stream.create`.
body.id       | string | _required_ | Unique ID of the stream to start.

## Response

You should get a Janus event with specified `transaction` and following body:

Name      | Type   | Default    | Description
--------- | ------ | ---------- | -----------
status    | int    | _required_ | If status is equal to 200 then everything went well otherwise an error occurred (see [error object](./api.error.md)).
