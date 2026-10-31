| ID | Category | Functional Requirement |
|---|---|---|
| FR-01 | Activity Tracking | The system shall identify the active window and its associated process. |
| FR-02 | Activity Tracking | The system shall record the start and end of each activity session. |
| FR-03 | Activity Tracking | The system shall store the start time, end time, duration, application, process identifier, and confidence level for each session. |
| FR-04 | Activity Tracking | The system shall store window titles if the user has permitted their collection. |
| FR-05 | Activity Tracking | The system shall detect the absence of user input and mark the system as idle after a configured period. |
| FR-06 | Activity Tracking | The system shall not count idle time as active application time. |
| FR-07 | Activity Tracking | The system shall account for screen locking, sleep, hibernation, shutdown, and device resume events. |
| FR-08 | Activity Tracking | The system shall correctly handle multiple monitors, virtual desktops, and rapid window switching. |
| FR-09 | Recovery | After an unexpected termination, the system shall recover the last session without counting time for any period for which reliable data is unavailable. |
| FR-10 | Classification | The user shall be able to group applications into categories. |
| FR-11 | Classification | The user shall be able to exclude selected applications and windows from tracking. |
| FR-12 | Classification | Changes to classification rules shall not modify the original event log. |
| FR-13 | Statistics | The system shall display statistics for a period selected by the user. |
| FR-14 | Statistics | The system shall display total active time, time by application and category, a timeline of application switches, and idle intervals. |
| FR-15 | Statistics | The system shall display verified, suspicious, and unreliable intervals separately. |
| FR-16 | Statistics | The user shall be able to search, filter, and sort records. |
| FR-17 | Management | The user shall be able to start and stop data collection. |
| FR-18 | Management | The system shall clearly display the current data collection status. |
| FR-19 | Management | The user shall be able to enable automatic application startup. |
| FR-20 | Management | The user shall be able to configure the data retention period and level of detail. |
| FR-21 | Management | The user shall be able to delete data for a selected period or delete all data. |
| FR-22 | Data Protection | The system shall perform a full or selective integrity check of the event log. |
| FR-23 | Data Protection | The system shall report the range of data checked and any integrity violations detected. |
| FR-24 | Data Protection | The system shall compare system and monotonic clock readings to detect changes to the system time. |
| FR-25 | Data Protection | The system shall record time jumps, observation gaps, unexpected restarts, and the absence of checkpoints for a configured period. |
| FR-26 | Data Protection | The system shall detect the restoration of an outdated database copy if a more recent trusted checkpoint is available. |
| FR-27 | Synchronization | When a server is used, the system shall transmit signed checkpoints and detect rollback of the history to an outdated state. |
| FR-28 | Diagnostics | The system shall display the database status, details of the latest checkpoint, and the result of the latest integrity check. |
