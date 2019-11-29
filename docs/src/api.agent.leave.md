# agent.leave

End plugin handle. Use it to notify Janus when a client goes away to clean up resources properly.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name         | Type   | Default    | Description
------------ | ------ | ---------- | -----------
body.method  | string | _required_ | Always `agent.leave`.

## Response

You should get a Janus event with specified `transaction` and following body:

Name      | Type   | Default    | Description
--------- | ------ | ---------- | -----------
status    | int    | _required_ | If status is equal to 200 then everything went well otherwise an error occurred (see [error object](./api.error.md)).
