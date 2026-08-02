# Command-line reference

`ssh-img-paste` uses the active profile unless `--profile ID` is provided. An explicit override applies to one command and does not change the active profile.

## Upload and remote files

```sh
ssh-img-paste
ssh-img-paste upload
ssh-img-paste --profile work upload
ssh-img-paste region
ssh-img-paste full
ssh-img-paste list
ssh-img-paste fetch NAME.png
ssh-img-paste clean
```

- `upload` sends a clipboard image, or uses the configured screenshot fallback when the clipboard has no image.
- `region` and `full` explicitly capture a screenshot before uploading.
- `list` prints `SIZE<TAB>NAME`, newest first.
- `fetch` downloads a validated PNG basename and prints its local path.
- `clean` deletes PNG uploads from the selected profile's configured remote directory.

## Profile discovery and selection

```sh
ssh-img-paste profiles
ssh-img-paste profile current
ssh-img-paste profile use work
ssh-img-paste profile inspect work
ssh-img-paste profile test work
```

`profiles` emits exactly four tab-separated columns: active marker, profile ID, display label, and SSH host. `profile inspect` emits stable `KEY<TAB>VALUE` rows. `profile test` is read-only and uses SSH batch mode.

## Profile management

```sh
ssh-img-paste profile create work \
  --label "Work SSH Host" \
  --host work-ssh \
  --remote-home /home/me \
  --remote-dir img-uploads

ssh-img-paste profile update work \
  --label "Work Server" \
  --shot-mode full \
  --restore-seconds 30

ssh-img-paste profile rename work client-a
ssh-img-paste profile delete old-client
ssh-img-paste profile delete client-a --switch-to fallback
```

Creating the first usable profile makes it active. Deleting an inactive profile does not require a replacement. Deleting the active profile requires `--switch-to ID`, and deleting the last usable profile is blocked.

The profile manager and CLI share validation and mutation rules. They store only app-owned destination fields; SSH ports, identities, jump hosts, keys, and credentials remain outside profile files.

## Exit behavior

Invalid input and unsafe loaded configuration fail before network or clipboard side effects. Connection and transfer failures return nonzero. Process timeouts return status `124`.

Use command output in scripts only where a machine-readable contract is documented above. Human-facing error text may change between releases.
