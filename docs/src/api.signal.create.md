# signal.create

Establishes WebRTC connection by SDP exchange.

After calling this method one may call [stream.create](api.stream.create.md) to start the stream
or [stream.read](api.stream.read.md) to subscribe to a stream. Mind specifying an SDP offer
with proper mode depending on the intent. In order to write the stream it must be `sendonly` or
`sendrecv`. In order to read the stream it must be `recvonly` or `sendrecv`.

The `agent_id` parameter is needed to associate an agent id with the session. It may be later
disconnected by calling [agent.leave](api.agent.leave.md) with the same `agent_id`.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name          | Type   | Default    | Description
------------- | ------ | ---------- | ---------------------
body.agent_id | string | _required_ | Agent ID associated with the session.
jsep.type     | string | _required_ | Always `offer`.
jsep.sdp      | string | _required_ | An SDP offer.

### Response

You should get a Janus event with specified `transaction` and following body:

Name      | Type   | Default    | Description
--------- | ------ | ---------- | -----------
status    | int    | _required_ | If status is equal to 200 then everything went well otherwise an error occurred (see [error object](./api.error.md)).
jsep.type | string | _required_ | Always `answer`
jsep.sdp  | string | _required_ | An SDP answer.
