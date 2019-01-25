# Overview

Janus Conference is a plugin for general purpose WebRTC server
Janus. Plugin handles media stream broadcasting from publishers
to subscribed watchers.

Plugin allow users to start and join streams by the means
of an [API](./api.md). Stream is meant to include some
video and audio tracks coming from separate sources - e.g.
there is a stream for webcamera and there is a stream for
screen capture software. 

Plugin configuration is descibed [here](./configuration.md).

## Auth

Plugin itself doesn't do any authentication/authorization so
some external manager is required for that.
This manager should receive SDP offers from clients, validate
and pass verified offers down to plugin, then return answers
back to clients.

## How to start hacking

You can find instructions on development environment setup
in README.md.
