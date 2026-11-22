# Command inventory

These are the commands advertised by the enabled areas. Run `ahjo <noun> <verb> --help` for arguments and behavior; availability depends on the node build and its enabled areas.

## agent

`ahjo agent create`, `ahjo agent delete`, `ahjo agent edit`, `ahjo agent list`, `ahjo agent show`.

## brain

`ahjo brain list-changes`, `ahjo brain list-collection`, `ahjo brain list-collections`, `ahjo brain list-files`, `ahjo brain read`, `ahjo brain write`, `ahjo brain search`, `ahjo brain send`, `ahjo brain read-texts`.

## config

`ahjo config show`.

## file

`ahjo file copy`, `ahjo file delete`, `ahjo file diff`, `ahjo file list`, `ahjo file search-paths`, `ahjo file create-folder`, `ahjo file move`, `ahjo file read`, `ahjo file list-recent`, `ahjo file resume`, `ahjo file search`, `ahjo file stat`, `ahjo file list-changes`, `ahjo file show-transfer`, `ahjo file write`.

## handoff

`ahjo handoff reply`, `ahjo handoff launch`, `ahjo handoff show`.

## harness

`ahjo harness create-hook`, `ahjo harness delete-hook`, `ahjo harness edit-hook`, `ahjo harness switch-hook`, `ahjo harness link`, `ahjo harness read`, `ahjo harness edit-setting`, `ahjo harness show-shape`, `ahjo harness show`, `ahjo harness sync`, `ahjo harness write`.

## host

`ahjo host show`.

## layout

`ahjo layout edit`, `ahjo layout show`.

## mission

`ahjo mission apply`, `ahjo mission list-archives`, `ahjo mission arrange`, `ahjo mission create`, `ahjo mission delete`, `ahjo mission edit`, `ahjo mission list-examples`, `ahjo mission export`, `ahjo mission list-history`, `ahjo mission list`, `ahjo mission note`, `ahjo mission notes`, `ahjo mission delete-archive`, `ahjo mission reply`, `ahjo mission list-repositories`, `ahjo mission resolve`, `ahjo mission run`, `ahjo mission show`, `ahjo mission validate`.

## project

`ahjo project search`, `ahjo project list`.

## run

`ahjo run abort`, `ahjo run approve`, `ahjo run delete`, `ahjo run land`, `ahjo run list`, `ahjo run pick`, `ahjo run reject`, `ahjo run resume`, `ahjo run show`, `ahjo run wait`.

## scheduled

`ahjo scheduled copy`, `ahjo scheduled create`, `ahjo scheduled delete`, `ahjo scheduled edit`, `ahjo scheduled list`, `ahjo scheduled pause`, `ahjo scheduled resume`, `ahjo scheduled run`, `ahjo scheduled show`.

## service

`ahjo service list`, `ahjo service read`, `ahjo service restart`, `ahjo service show`, `ahjo service start`, `ahjo service stop`.

## session

`ahjo session copy`, `ahjo session list-folders`, `ahjo session send-keys`, `ahjo session delete`, `ahjo session launch`, `ahjo session list-launch-flags`, `ahjo session list`, `ahjo session edit-mouse`, `ahjo session read`, `ahjo session edit`, `ahjo session search`, `ahjo session mark-seen`, `ahjo session send`, `ahjo session list-windows`.

## view

`ahjo view open`, `ahjo view send`, `ahjo view show` (the connected apps are under `apps` in JSON).

## issue

`ahjo issue create`, `ahjo issue list`, `ahjo issue search`, `ahjo issue show`, `ahjo issue search-snapshot`, `ahjo issue read-snapshot`. The snapshot commands use the last tracker snapshot.
