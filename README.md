# Janus Conference

A Janus Gateway plugin implementing Conference streams.


### How To Use

To build and start playing with the plugin,
execute following shell commands:

```bash
## Create the application configuration and environment files from samples
cp docker/janus.plugin.conference.environment.sample docker/janus.plugin.conference.environment

# Build and run Janus instance with plugin
export COMPOSE_FILE=docker/docker-compose.yml
docker-compose up
```

### How to run example

```bash
cd examples
npm install
npm run build
open index.html
```

Click `Connect` & `Start translation` button (page should ask for permission
to use web camera) then open page again in another tab and click `Connect` &
`Join translation`. On publisher page you should see local stream
on the left and on listener page you should see remote stream on
the right.


## License

The source code is provided under the terms of [the MIT license][license].

[license]:http://www.opensource.org/licenses/MIT
[travis]:https://travis-ci.com/netology-group/janus-conference?branch=master
[travis-img]:https://travis-ci.com/netology-group/janus-conference.png?branch=master
