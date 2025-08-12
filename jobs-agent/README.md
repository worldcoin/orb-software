# Orb Jobs Agent (orb-jobs-agent)

The Orb Jobs Agent is a process to provide remote execution of prescribed commands. Commands are invoked by incoming requests and the
functionality of the command is provided completely by the Orb implementation.

### Taxonomy

- `orb-jobs-agent`: A systemd service that runs on the orb, executing a `JobExecution`(s) sent by `fleet-cmdr`
- `fleet-cmdr`: A backend service owned by the Fleet Management team that sends `JobExecution`(s) to the orb. 
- `JobNotify`: A notification sent from `fleet-cmdr` to the `orb-jobs-agent` that new jobs are pending.
- `JobRequestNext`: A request from the `orb-jobs-agent` to the `fleet-cmdr` to send the next `JobExecution`.
- `JobExecution`: A specific command to execute by the current Orb
- `JobExecutionUpdate`: A result from the `orb-jobs-agent` describing the outcome or progress of a command execution
- `JobCancel`: A cancellation request to stop a running job in `orb-jobs-agent`.

### fleet-cmdr's role from the orb-jobs-agent perspective
- `fleet-cmdr` provides an interface for users to enqueue new jobs to its internal queue
- when a new job is enqueued in `fleet-cmdr`, it sends a `JobNotify` to `orb-jobs-agent`
- when `fleet-cmdr` receives a `JobRequestNext` from `orb-jobs-agent`, it sends the first job (that is not already 
in progress) from its queue
- when `fleet-cmdr` receives a `JobExecutionUpdate` from `orb-jobs-agent`, it removes that job from its internal queue

### orb-jobs-agent role
- on startup, `orb-jobs-agent` requests `fleet-cmdr` for a new job with `JobRequestNext`
- when `orb-jobs-agent` receives a `JobNotify` from `fleet-cmdr` it requests a new job with a `JobRequestNext`
- when `orb-jobs-agent` receives a `JobExecution` from `fleet-cmdr` it executes a job
- on completion of a job, `orb-jobs-agent` reports back to `fleet-cmdr` with a `JobExecutionUpdate`
- on completion of a job, `orb-jobs-agent` requests `fleet-cmdr` for a new job with a `JobRequestNext`

### notes
- jobs can be cancelled
- some jobs can be run in parallel, other jobs not

