# rtc.read

Read a real-time connection in order to initialize signaling phase and receive media.

## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).

### Parameters

Name         | Type   | Default    | Description
------------ | ------ | ---------- | -----------
body.method  | String | _required_ | Always `rtc.read`
body.room_id | String | _required_ | Unique ID of the stream you want to start. This string is used to group publishers and subscribers. **It's up to you to generate these IDs and ensure their consistency.**
jsep.type    | String | _required_ | Always `offer`
jsep.sdp     | String | _required_ | An SDP offer

## Response

If everything went well you should get a Janus event with specified `transaction` and following body:

Name   | Type   | Default    | Description
------ | ------ | ---------- | -----------
result | String | _required_ | Always `ok`
