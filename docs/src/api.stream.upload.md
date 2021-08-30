# stream.upload

Upload a mjr dumps to s3 storage.


## Request

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).


### Parameters

Name         | Type   | Default    | Description
------------ | ------ | ---------- | -----------
body.method  | string | _required_ | Always `stream.upload`.
body.id      | string | _required_ | Unique ID of the stream you want to start. This string is used to group publishers and subscribers. **It's up to you to generate these IDs and ensure their consistency.**
body.backend | string | _required_ | Destination S3 backend.
body.bucket  | string | _required_ | Destination S3 bucket.


## Response

You should get a Janus event with specified `transaction` and following body:

Name           | Type                   | Default    | Description
-------------- | ---------------------- | ---------- | -----------
status         | Int                    | _required_ | If status is equal to 200 then everything went well otherwise an error occurred (see [error object](./api.error.md)).
mjr_dumps_uris | Array of Strings       | []         | An array of uris to janus dump files


## Example

```bash
CONFERENCE_ACCOUNT_ID='conference.svc.example.org'
JANUS_ACCOUNT_ID='janus-gateway.svc.example.org'
JANUS_SESSION_ID='6467722327202552'
JANUS_HANDLE_ID='6383585627302052'

mosquitto_pub \
    -i "v1.mqtt3/service-agents/test-1.${CONFERENCE_ACCOUNT_ID}" \
    -t "agents/alpha.${JANUS_ACCOUNT_ID}/api/v1/in/${CONFERENCE_ACCOUNT_ID}" \
    -m '{"payload": "{\"janus\":\"message\", \"session_id\": '${JANUS_SESSION_ID}', \"handle_id\": '${JANUS_HANDLE_ID}', \"body\": {\"method\": \"stream.upload\", \"id\": \"'${RTC_ID}'\", \"backend\": \"'${BACKEND}'\", \"bucket\": \"origin.webinar.'${AUDIENCE}'\"}, \"transaction\": \"ignore\"}"}'
```
