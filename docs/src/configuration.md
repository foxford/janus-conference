# Configuration

Plugin expects a TOML-file located at the following path -
`${JANUS_INSTALL_DIR}/etc/janus/janus.plugin.conference.toml`.

Configuration sample:

```toml
[recording]
root_save_directory = "records/"
```

## `recording` section

Parameter           | Default value | Description
------------------- | ------------- | -----------
root_save_directory | *required*    | Root directory to which all the records are saved.
