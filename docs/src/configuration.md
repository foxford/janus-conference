# Configuration

Plugin expects a TOML-file located at the following path -
`${JANUS_INSTALL_DIR}/etc/janus/janus.plugin.conference.toml`.

Configuration sample:

```toml
[recordings]
directory = "recordings/"
enabled = true
```

## `recordings` section

Parameter | Default value | Description
--------- | ------------- | -----------
directory | *required*    | Directory to which all the records are saved.
