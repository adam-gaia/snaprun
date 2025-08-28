# snaprun

Use any cli command's output for snapshot tests

## Usage

### Creating snapshots

Each snapshot is defined by a directory <name> containing

- snapshot.toml (required):

```toml
command = "hello-world" # Required, command to run
args = ["foo", "bar"] # Optional, command's args
code = 0 # Optional, expected output code. Defaults to zero if omitted
stdout = "Hello, World!" # Optional, stdout text to compare against
stderr = "" # Optional, stderr text to compare against
#stdin = "" # Optional, stdin to feed program
timeout = 3000 # Optional, number in milliseconds to kill the program after
```

The stdout, stderr, stdin, may also be provided via their own files (useful when dealing with lots of content).

- stderr.txt (optional): stderr text to compare against
- stdout.txt (optional): stdout text to compare against
- stdin.txt (optional): stdin to feed program

Note that the exit code is always checked, however stdout/err are only compared if provided.
E.g. a missing exit code is assumed to be zero, but omitting both the `stdout` field and the `stdout.txt` file means the test will pass regardless of the stdout.
If you wish verify your snapshot test returned no stdout, set `stdout = ""` with the empty string, or create an empty `stdout.txt` file.

Snaprun will create/update these snapshot files automatically, but they can always be manually created/updated.

- 'snaprun new' executes the provided command and saves their exit code, stdout, etc.

```console
$ snaprun new --outdir ./snapshots "hello-test" --command "hello-world" --stdin="" --timeout 250
Created snapshot 'hello-test'
  ./snapshots/hello-test/snapshot.toml
  ./snapshots/hello-test/stdout.txt
  ./snapshots/hello-test/stderr.txt
  ./snapshots/hello-test/stdin.txt

```

- 'snaprun update' executes the provided command and overwrites the exit code, stdout, etc.

```console
$ snaprun update "hello-test" --new-name "goodbye-test" --command "goodbye-world" --outdir ./snapshots
Updated snapshot 'goodbye-test'
  name: 'hello-test' -> 'goodbye-test'
  command: 'hello-world' -> 'goodbye-world'
  unset timeout = '250'
  unset stdin = ''
  stdout: 'Hello, World![NEWLINE]' -> 'Goodbye, World![NEWLINE]'


```

<!-- TODO: why do we need the double newline above for trycmd to pass? -->

### Running tests

Run tests with `snaprun run <snapshots dir>`

```console
$ snaprun run ./example-snapshots
[PASS] hello-test
[FAIL] diff-example
  stdout mismatch:
---- left:  expected
++++ right: got
   1     - foo bar baz
       1 + Hello, World!
       2 + 


[FAIL] xdg-ninja
  command 'xdg-ninja' not found

```

## Motivation

I periodically run [xdg-ninja](https://github.com/b3nj5m1n/xdg-ninja) to see if there is shit I can move out of my $HOME dir.
One day I had a thought that it would be nice to have a cron job or systemd timer script to alert me if a new program put something in my home dir.
I only wanted to be alerted when new files were added to my home dir, without bothering me about the existing offernders I'd either decided to tolerate or hadn't gotten around to moving yet.
I realized that I wanted a weekly **regression/snapshot** test for xdg-ninja.
That soon turned into a tool-agnostic snapshot test runner.

(Wouldn't it be funny if this tool only used ~/.snaprun for configuration?)

## Credits

This tool is heavily inspired by [trycmd](https://docs.rs/trycmd/latest/trycmd/)
I've always wanted trycmd to be a standalone tool, and snaprun is essentially that.
I sort of watered-down trycmd's test case syntax/files to get an MVP out quick.
Eventually, I'd like to fully support trycmd test cases (e.g. write snaprun tests with *.trycmd/*.toml, and supporting files from \*.in, \*.out, etc)
