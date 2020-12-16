# Changelog

## v0.8.4 (December 16, 2020)

### Features
- Add lock file for records uploading & upload mjr dumps ([c6f4b5f](https://github.com/netology-group/janus-conference/commits/c6f4b5fbaa3d02571992f600140155baca8d3615))
- Log stream id ([3445ae3](https://github.com/netology-group/janus-conference/commits/3445ae3df36df45451c29a096efd5230f7f98fe3))


## v0.8.3 (October 26, 2020)

### Fixes
- Fix memory leak and wrong log messages ([2ae630e](https://github.com/netology-group/janus-conference/commits/2ae630eaea91a63691492638a64398edc8394707))


## v0.8.2 (October 21, 2020)

### Features
- Add more logging on stream removing ([8e8d782](https://github.com/netology-group/janus-conference/commits/8e8d782a6e6cbcf2c5368dfbe71f01e24ca2a213))


## v0.8.1 (October 20, 2020)

### Fixes
- Fix aws credentials config ([0570d24](https://github.com/netology-group/janus-conference/commits/0570d24d755bde6a76a11080d1a75e7fb253ab63))


## v0.8.0 (October 20, 2020)

### Features
- Add contextual JSON logging ([4471cea](https://github.com/netology-group/janus-conference/commits/4471ceabbf4b8aab450fc931133104206efb75f0))

### Changes
- Add backend selection for uploading ([1dbde12](https://github.com/netology-group/janus-conference/commits/1dbde12bdfa48071ea9cc45ea259025826c04097))


## v0.7.5 (October 1, 2020)

### Changes
- Reduce uploading logging verbosity ([98980a1](https://github.com/netology-group/janus-conference/commits/98980a1369120928a7ba3a237dcd7d873060db85)
- Limit aws-cli bandwidth usage ([5c1fb1f](https://github.com/netology-group/janus-conference/commits/5c1fb1f665c3ce3611b8d313eec7ab97b6bcf93d))


## v0.7.4 (September 29, 2020)

### Changes
- Update Janus Gateway ([2204bfe](https://github.com/netology-group/janus-conference/commits/2204bfe9699ecb711b532ea2ff19092b7278e1e6))
- Update Paho ([e74f7ab](https://github.com/netology-group/janus-conference/commits/e74f7ab9971448816822375a012d382e0c810d6f))

### Fixes
- Disconnect subscribers on publisher disconnect ([d4dee59](https://github.com/netology-group/janus-conference/commits/d4dee59e82d140bd60e39994b0c39a7e17f7dfd4))


## v0.7.3 (September 14, 2020)

### Changes
- Upgrade Janus Gateway ([5c3e86c](https://github.com/netology-group/janus-conference/commit/5c3e86c6a26189a0d3ab800a568d1a886484e302))


## v0.7.2 (August 31, 2020)

### Fixes
- Add subscribers disconnection on stream.upload ([8dc66b1](https://github.com/netology-group/janus-conference/commit/8dc66b1823cdf9279500ff4808748a7f3f6d16cc))
- Skip empty segments on concat ([cf82fab](https://github.com/netology-group/janus-conference/commit/cf82fab833e4b4c4c0767deed69a52d0921ce738))
- Remove artifacts from possible previous run ([cea6cd5](https://github.com/netology-group/janus-conference/commit/cea6cd534feadc50212236ad8ab70a93adf954a3))
- Skip segments with N/A duration ([2806f18](https://github.com/netology-group/janus-conference/commit/2806f18ce8ee62ffa5ff9400732c32e3ae7911b8))


## v0.7.1 (August 7, 2020)

### Features
- Add recordings deletion config option ([029bbe3](https://github.com/netology-group/janus-conference/commit/029bbe36e1ec29dccbd130745ceb80035c0775c6))

### Changes
- Upgrade janus from the main repo ([27e0d3e](https://github.com/netology-group/janus-conference/commit/27e0d3e284f292ceed06e59fa61dcca3bc0d6292))


## v0.7.0 (July 15, 2020)

### Changes
- Upgrade debian, janus & deps ([a580d1c](https://github.com/netology-group/janus-conference/commit/a580d1c8b02081b5aff78fbdd546c8a590ffabe2))
- Limit publisher bitrate with REMB & SDP ([e781235](https://github.com/netology-group/janus-conference/commit/e781235b67522e93d06ae348c0d4ed07b0217120))
- Switch to VP8 ([28b650f](https://github.com/netology-group/janus-conference/commit/28b650f18d2f30fe854aca17d951a44bc963b66f))
- Replace gstreamer with janus recorder ([5ae59e5](https://github.com/netology-group/janus-conference/commit/5ae59e527f74486b67e415552bf97d035568d734), [9d97054](https://github.com/netology-group/janus-conference/commit/9d970546a11b7094468d8f4f5698cb1912f4f6b1))
- Upgrade janus & increase MQTT client limits ([abf155d](https://github.com/netology-group/janus-conference/commit/abf155d2c4af31b07b35a73c5b0b52cd48a099bb))


## v0.6.3 (June 4, 2020)

### Changes
- Switch to anyhow from failure ([f4e33a5](https://github.com/netology-group/janus-conference/commit/f4e33a51dc7b3a75be4fef2b5116bc4221396f08))
- Add rtpjitterbuffer to recording pipeline ([f23df50](https://github.com/netology-group/janus-conference/commit/f23df50aad6f69e05bbdb0d6775b9743d1176cf7))


## v0.6.2 (April 30, 2020)

### Changes
– Upgrade Janus fork ([667476a](https://github.com/netology-group/janus-conference/commit/667476a49dbd4bbeefccd531a9621149bec2776d))


## v0.6.1 (April 14, 2020)

### Changes
– Upgrade Janus fork ([e12db02](https://github.com/netology-group/janus-conference/commit/e12db020738fe51ce5dccd246e9f6428b64683c6))

### Fixes
- Fix uploading ongoing stream ([2fd5d3b](https://github.com/netology-group/janus-conference/commit/2fd5d3b6b021998df80b31cf181cfd8f3583bd09))
- Fix S3 uploading to Yandex ([80e4106](https://github.com/netology-group/janus-conference/commit/80e4106dfcf21b4ee425eabb18650edb13dcf1d3))

## v0.6.0 (January 15, 2020)

### Features

- Use request/response pattern ([bd19966](https://github.com/netology-group/janus-conference/commit/bd19966d2b8eac0c3f539aeb1f2d2bcd8e17f9fe))


## v0.5.0 (December 20, 2019)

### Features

- Delete recording source after uploading ([1b9028c](https://github.com/netology-group/janus-conference/commit/1b9028cd127140b56d958bd64c3049b599ce5151))
- End handle on WebRTC hangup ([c4f9967](https://github.com/netology-group/janus-conference/commit/c4f99674f4e98d68fa26f91df3249296bab55fce))
- Add `agent.leave` endpoint ([ddb1b8f](https://github.com/netology-group/janus-conference/commit/ddb1b8f2467b4a029b52fa2a424bc2166598fe52))
- Switch to v2 connection ([dd4a558](https://github.com/netology-group/janus-conference/commit/dd4a558c158acc5a45576571922219ae51e31885))

### Changes

- Thread safety overhaul ([be96874](https://github.com/netology-group/janus-conference/commit/be968741ae15907068f16d0f81f4bf6a6191b2b6))
- Rewrite example client ([37f8dd6](https://github.com/netology-group/janus-conference/commit/37f8dd647b9915b450b5d1620b8ae89114eea023))

### Fixes

- Remove all agent handles on `agent.leave` ([b54ff0e](https://github.com/netology-group/janus-conference/commit/b54ff0ebf92244e61b535fa5d3293c8bb5141338))


### Dependencies

- Upgrade to futures 0.3 ([d25cc3b](https://github.com/netology-group/janus-conference/commit/d25cc3bcbdb33e9cfd4be15c91f86c901d7d469b))


## v0.4.0 (September 3, 2019)

### Changes

- Major refactor of message handling; use svc-error crate ([a47804b](https://github.com/netology-group/janus-conference/commit/a47804b95e24fb54accd3ddf146f759c318a3e17), [82b53eb](https://github.com/netology-group/janus-conference/commit/82b53eb396794e9137f12830454e93fa7cd0881c))
- Return 404 on missing recording ([99a081e](https://github.com/netology-group/janus-conference/commit/99a081eb306ff0fed962968c0a21fe8cba11d947))
- Add Sentry error tracking ([35df5dc](https://github.com/netology-group/janus-conference/commit/35df5dc210b960ab5ba1f46581cec494535e26d7))

### Fixes

-  Upgrade Janus to a version with MQTT transport automatic reconnection fix ([b7ec792](https://github.com/netology-group/janus-conference/commit/b7ec792f85851a8f79c881603255e3b438f35d93))


## v0.3.1 (August 7, 2019)

### Fixes

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

### Fixes

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

### Fixes

- Fix phantom streams ([db80594](https://github.com/netology-group/janus-conference/commit/db80594ce334f80d9493e3212f8a586561fc33d7))
- Fix memory leak ([f6f143f](https://github.com/netology-group/janus-conference/commit/f6f143fa46435bbdfb593b124b16c17767014338))

### Dependencies

- Upgrade Janus ([2d3ed8a](https://github.com/netology-group/janus-conference/commit/2d3ed8a3068a9c5f374623f2df63ada2f35498da))
- Upgrade PAHO MQTT client ([aee1557](https://github.com/netology-group/janus-conference/commit/aee1557e1d884eb132634540a5286bac24b51b59))
- Update crates ([14ade6b](https://github.com/netology-group/janus-conference/commit/14ade6b9e1403fcebd662430b5539b23699b9e5d))


## v0.1.0 (Dec 8, 2018)

Initial release
