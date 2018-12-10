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

## How to start hacking

You can find instructions on development environment setup
in README.md.
