# Changelog

## v0.3.1 (August 7, 2019)

### Bugfixes

- Switch back to Debian due to segfault in libnice ([7ae406b](https://github.com/netology-group/janus-conference/commit/7ae406b69b75378879993798f49970e60b46e9a2))

## v0.3.0 (August 5, 2019)

### Changes

- Return relative timestamps in `time` + absolute `started_at` in `stream.upload` response ([571db91](https://github.com/netology-group/janus-conference/commit/571db917e89a98145b4a1db3ce8f8d3843a5611b))

- Switch to MQTT v5 ([5eb7756](https://github.com/netology-group/janus-conference/commit/5eb7756d25ec7c188f2d317ae455871e1b8a6ff6))

- Switch to Alpine Linux

## v0.2.1 (June 13, 2019)

### Changes

- Vacuum inactive publishers to allow translation after refresh ([04804b4](https://github.com/netology-group/janus-conference/commit/04804b42e473538489f90b109f45ff8ab4b92993))

- Rescale in recording pipeline to avoid peak load ([f662c27](https://github.com/netology-group/janus-conference/commit/f662c274b97327552a28f6c8dc3bd68d260f4cd2))

- Concat recordings with ffmpeg to not to hang on corrupted videos ([39c1897](https://github.com/netology-group/janus-conference/commit/39c18979c2361ab0c1110b20fe6ea4c66a7d967c))

- Add videoconvert element to the pipeline to align the framerate ([2fca86e](https://github.com/netology-group/janus-conference/commit/2fca86e58da377e5c4652f986326a41f41fb74c3))

### Bugfixes

- Fix bad Janus upgrade ([3d51732](https://github.com/netology-group/janus-conference/commit/3d5173298b5f145d8c2967de350b793e2ab246c7))

## v0.2.0 (June 7, 2019)

### Features

- Implement reverse signaling for subscribers ([11cb98a](https://github.com/netology-group/janus-conference/commit/11cb98aedfbb302ecc55af966202fd34d563d7e0))
- Add stream recording ([d8a944a](https://github.com/netology-group/janus-conference/commit/d8a944a4dbf9ffc4c0aa99354b490189d3266fd1), [66a1053](https://github.com/netology-group/janus-conference/commit/66a1053cfcabde60a4d07589cd7316eb2c952184), [82f5c44](https://github.com/netology-group/janus-conference/commit/82f5c447118b2b2e2b616eb52196ad6005fa2e7b), [41ebc0a](https://github.com/netology-group/janus-conference/commit/41ebc0a3369b4e95a1c4f397ba4a01e218422297), [9fa753c](https://github.com/netology-group/janus-conference/commit/9fa753cd0f6352a1a6e871c36cb8c49ad16858ca))
- Add start/stop timestamps return ([93642ed](https://github.com/netology-group/janus-conference/commit/93642edaef5c5cf21559743879443703238bb8c1), [13500d5](https://github.com/netology-group/janus-conference/commit/13500d57ef57f01a9341b00a74e5fb399430f5a4), [7a8546d](https://github.com/netology-group/janus-conference/commit/7a8546d18735e9c3d2fdd4ce8403af35a628641e), [a30b337](https://github.com/netology-group/janus-conference/commit/a30b3375f8bfa80e0dc13d5e5509e411ff4dec6e))

### Changes

- Rename API methods ([62ce01c](https://github.com/netology-group/janus-conference/commit/62ce01c18af4050359aeaf933fa2636619318087))
- Improve error handling ([bec534c](https://github.com/netology-group/janus-conference/commit/bec534c247b026e890c6ad1b13dcf40a7a03079b))
- Pin exact codecs ([e64f2ce](https://github.com/netology-group/janus-conference/commit/e64f2ce08cc1c7d5253ad51a5b063437a119b670))
- Remove H264 profile ([3156cea](https://github.com/netology-group/janus-conference/commit/3156cea959e7310ea2e16b7933083dcdfa1ab876))
- Return errors to client ([b50e9fd](https://github.com/netology-group/janus-conference/commit/b50e9fdbc1a0221b1d6f01fe9816aeddc8e54bf1))
- Redirect subscribers to new publishers in stream ([51bf7d7](https://github.com/netology-group/janus-conference/commit/51bf7d7bf5b8cf929f8a5f6450f8375b39a9e6b4))
- Remove ack message ([b493cc4](https://github.com/netology-group/janus-conference/commit/b493cc4026c4b2c6f710e95c087bdf354043ddf0))
- Make errors to be in accordance with spec ([e1fb6ea](https://github.com/netology-group/janus-conference/commit/e1fb6ea6bc282a672ff2a83e301780e90697556f))
- Cast videos to common format on concat ([f37f340](https://github.com/netology-group/janus-conference/commit/f37f340eefddf97be7862e9cf09701a8e4e7717f))

### Bugfixes

- Fix phantom streams ([db80594](https://github.com/netology-group/janus-conference/commit/db80594ce334f80d9493e3212f8a586561fc33d7))
- Fix memory leak ([f6f143](https://github.com/netology-group/janus-conference/commit/f6f143fa46435bbdfb593b124b16c17767014338))

### Dependencies

- Upgrade Janus ([2d3ed8a](https://github.com/netology-group/janus-conference/commit/2d3ed8a3068a9c5f374623f2df63ada2f35498da))
- Upgrade PAHO MQTT client ([aee1557](https://github.com/netology-group/janus-conference/commit/aee1557e1d884eb132634540a5286bac24b51b59))
- Update crates ([14ade6b](https://github.com/netology-group/janus-conference/commit/14ade6b9e1403fcebd662430b5539b23699b9e5d))

## v0.1.0 (Dec 8, 2018)

Initial release
