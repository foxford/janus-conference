# API

Janus server requires client to [initialize session and get plugin handle](https://janus.conf.meetecho.com/docs/rest.html)
before interaction with plugin.

Every message described here should be in `body` key of Janus message.
Example:

```json
{
    "janus": "message",
    "transaction": "example-transaction",
    "body": {
        "method": "rtc.create",
        "room_id": "1234"
    },
    "jsep": ...
}
```

1. Create stream

    - `method`: `rtc.create`
    - `room_id`: any `String`. Unique ID of the stream you want to start.
    This string is used to group publishers and subscribers.
    **It's up to you to generate these IDs and ensure their consistency.**

    You should send JSEP with SDP offer in order to start signaling
    process for publisher.

2. Join stream

    - `method`: `rtc.read`
    - `room_id`: any `String`. Unique ID of the stream you want to join.
    This string is used to group publishers and subscribers.
    **It's up to you to generate these IDs and ensure their consistency.**

    You should send JSEP with SDP offer in order to start signaling
    process for subscriber.
