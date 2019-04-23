# Error object

Name   | Type   | Default    | Description
------ | ------ | ---------- | -----------
type   | String | _required_ | Failed operation name.
title  | String | _required_ | Human-readable description of failure.
status | Int    | _required_ | Whether 500, 400 or 404.
detail | String | _required_ | Detailed description of an error.

## Status meaning

* 500 - unexpected internal error.
* 400 - badly formatted request.
* 404 - entity is not found.
