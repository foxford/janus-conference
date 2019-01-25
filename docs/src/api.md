# API

Janus server requires client to [initialize session and get plugin handle](https://janus.conf.meetecho.com/docs/rest.html)
before interaction with plugin.

- [Error object](./api.error.md)

- [stream.create](./api.stream.create.md)
- [stream.read](./api.stream.read.md)
- [stream.upload](./api.stream.upload.md)


## Common properties

### Properties

Name        | Type   | Default    | Description
----------- | ------ | ---------- | -----------
janus       | String | _required_ | Always `message`
session_id  | String | _required_ | You get this ID after Janus session initialization
handle_id   | String | _required_ | You get this ID when you attach your session to plugin
transaction | String | _required_ | The same value will be in a response
body        | Object | _required_ | Request payload. See specific API methods
jsep        | Object | {}         | JSEP with an SDP offer or answer
