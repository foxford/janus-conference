# Overview

Janus Conference is a plugin for general purpose WebRTC server
Janus. Plugin handles media stream broadcasting from publishers
to subscribed watchers.

Plugin allow users to start and join stream rooms by the means
of an [API](./api.md). Each stream publisher is assigned its own room
where you can send subscribers. So if you have 2 separate video
streams you want to combine in a single client page you need to
subscribe the client to 2 separate rooms.

## Auth

Plugin itself doesn't do any authentication/authorization so
some external manager is required for that.
This manager should receive SDP offers from clients, validate
and pass verified offers down to plugin, then return answers
back to clients.

## How to use

In order to start Janus instance with this plugin included run
following commands:

```bash
bash docker/dev.run.sh
open examples/conference/index.html
```

Click `Start translation` button (page should ask for permission
to use web camera) then open page again in another tab and click
`Join translation`. On publisher page you should see local stream
on the left and on listener page you should see remote stream on
the right.
