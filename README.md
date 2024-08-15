### DIRWATCH

The simplest hot module reloading there is:

1. Watch the specified directory for any file changes.
2. On detecting a change, run the provided command.
3. Finally, it triggers a page refresh on the client to display the latest changes.

### Usage

```shell
dirwatch -watch <dir to watch> -serve <dir to serve> -run '<command to run>' -port <port to listen to>
```

### Example

To watch the `src` directory, serve files from the `dist` directory, and run a build command on file changes:

```shell
dirwatch -watch src -serve dist -run 'npm run build'
```
