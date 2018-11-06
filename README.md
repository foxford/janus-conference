# Janus Conference

A Janus Gateway plugin implementing Conference rooms.



### How To Use

To build and start playing with the plugin,
execute following shell commands within different terminal tabs:

```bash
## Building the image locally
docker build -t sandbox/janus-conference -f docker/Dockerfile .
## Running a container with Janus Gateway and the plugin
docker run -ti --rm sandbox/janus-conference
```



## License

The source code is provided under the terms of [the MIT license][license].
