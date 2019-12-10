# agent.leave

Notify Janus that an agent is left to clean up his handle in case when he doesn't hang up explicitly.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name            | Type   | Default    | Description
--------------- | ------ | ---------- | -----------
body.method     | string | _required_ | Always `stream.read`.
body.agent_id   | string | _required_ | Agent id to identify who is left.

## Response

You should get a Janus event with specified `transaction` and following body:

Name      | Type   | Default    | Description
--------- | ------ | ---------- | -----------
status    | int    | _required_ | If status is equal to 200 then everything went well otherwise an error occurred (see [error object](./api.error.md)).

