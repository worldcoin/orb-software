# Orb Jobs Agent (jobs-agent)

The Orb Jobs Agent is a process to provide remote execution of prescribed commands. Commands are invoked by incoming requests and the functionality of the command is provided completely by the Orb implementation.

## Jobs Agent Operation

The jobs agent client and server exchange job request messages and execution results in a manner described below.

### Taxonomy

- **Job**: A set of commands issued to one or more Orbs. Jobs are created by external applications such as the Orb Fleet CLI tool.
- **Job Notify**: A notification sent from the Server to the Client that new jobs are pending.
- **Job Request**: A request from the Client to the Server to send the next Job Execution.
- **Job Execution**: A specific command to execute by the current Orb
- **Job Update**: A result from the Orb describing the outcome or progress of a command execution

### Protocol

The Jobs Agent connects to the Orb Relay on startup, then follows this protocol:

1. Client send a **Job Request**
2. If any **Job Executions** are pending for the Orb the next one is sent to the Client
3. The Client performs the command and returns a **Job Update** back to the Service to indicate the outcome or progress of the command.
4. The Server receives the **Job Update** it records the status and provided output. If the **Job Update** status is:
   - IN PROGRESS: record the state any progress output. Wait for further **Job Updates**
   - SUCCESS: update the current **Job** as succeeded for this Orb. Send the next **Job Execution** for the current **Job** (if available), or from the next **Job** (if available) to the client
   - FAILED: update the current **Job** as 'failed' for this Orb. Send the next **Job Execution** from the next **Job** (if available)
5. Once all pending **Job Executions** are performed, the client will wait for the next **Job Notify** from the Server. 
6. On receiving **Job Notify** go to step 1.
