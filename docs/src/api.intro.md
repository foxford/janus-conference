# Intro

In order to deal with plugin's API a Janus Gateway session and handle objects should be created. Their identifiers will be used in every request of the plugin's API.

You can send a request over [any configured Janus transport](https://janus.conf.meetecho.com/docs/rest.html).


## Example

```bash
CONFERENCE_ACCOUNT_ID='conference.svc.example.org'
JANUS_ACCOUNT_ID='janus-gateway.svc.example.org'

## Creating a session
mosquitto_pub \
    -i "v1.mqtt3/service-agents/alpha.${CONFERENCE_ACCOUNT_ID}" \
    -t "agents/alpha.${JANUS_ACCOUNT_ID}/api/v1/in/${CONFERENCE_ACCOUNT_ID}" \
    -m '{"payload": "{\"janus\":\"create\", \"transaction\": \"foobar\"}"}'

## Creating a handle
JANUS_SESSION_ID='3244759732098420'
mosquitto_pub \
    -i "v1.mqtt3/service-agents/alpha.${CONFERENCE_ACCOUNT_ID}" \
    -t "agents/alpha.${JANUS_ACCOUNT_ID}/api/v1/in/${CONFERENCE_ACCOUNT_ID}" \
    -m '{"payload": "{\"janus\":\"attach\", \"session_id\": '${JANUS_SESSION_ID}', \"plugin\": \"janus.plugin.conference\", \"transaction\": \"foobar\"}"}'
```

```bash
## Subscribing to Janus Gateway responses topic
mosquitto_sub \
    -i "v1.mqtt3/service-agents/test-3.${CONFERENCE_ACCOUNT_ID}" \
    -t "apps/${JANUS_ACCOUNT_ID}/api/v1/responses" | jq '.'

{
  "payload": "{\"janus\":\"success\",\"transaction\":\"ignore\",\"data\":{\"id\":3244759732098420}}",
  "properties": {
    "account_label": "janus-gateway",
    "agent_label": "alpha",
    "audience": "svc.example.org",
    "type": "event"
  }
}

{
  "payload": "{\"janus\":\"success\",\"session_id\":3244759732098420,\"transaction\":\"ignore\",\"data\":{\"id\":252626787117466}}",
  "properties": {
    "account_label": "janus-gateway",
    "agent_label": "alpha",
    "audience": "svc.example.org",
    "type": "event"
  }
}
```
