# Janus Conference

A Janus Gateway plugin implementing Conference rooms.



### How To Use

To build and start playing with the plugin,
execute following shell commands:

```bash
# Build and run Janus instance with plugin
bash docker/run.sh
```

### How to run echo example

```bash
# Open example page in browser
open examples/echo/index.html
```

Then click `Start` (page should ask for permission to use
web camera and microphone) and `Call` after that. You should
see both local and echoed video stream.

### How to build an image for deploy

```bash
docker build -t netologygroup/janus-gateway --target deploy -f docker/Dockerfile .
```


## License

The source code is provided under the terms of [the MIT license][license].

[license]:http://www.opensource.org/licenses/MIT
