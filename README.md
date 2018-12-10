# Janus Conference

[![Build Status][travis-img]][travis]

A Janus Gateway plugin implementing Conference rooms.



### How To Use

To build and start playing with the plugin,
execute following shell commands:

```bash
# Build and run Janus instance with plugin
bash docker/dev.run.sh
```

### How to run conference example

```bash
# Open example page in browser
open examples/conference/index.html
```

Click `Start translation` button (page should ask for permission
to use web camera) then open page again in another tab and click
`Join translation`. On publisher page you should see local stream
on the left and on listener page you should see remote stream on
the right.


## License

The source code is provided under the terms of [the MIT license][license].

[license]:http://www.opensource.org/licenses/MIT
[travis]:https://travis-ci.com/netology-group/janus-conference?branch=master
[travis-img]:https://travis-ci.com/netology-group/janus-conference.png?branch=master