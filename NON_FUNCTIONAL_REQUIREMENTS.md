
| ID | Category | Non-Functional Requirement |
|---|---|---|
| NFR-01 | Performance | Event writes shall be performed asynchronously or in batches and shall not noticeably interfere with the user's work. |
| NFR-02 | Reliability | The system shall maintain data consistency after an unexpected termination or power failure. |
| NFR-03 | Reliability | Operations that write related data shall be performed transactionally. |
| NFR-04 | Reliability | After a restart, the system shall not count an already stored interval again. |
| NFR-05 | Reliability | A user interface failure shall not stop the background collector. |
| NFR-06 | Reliability | Corruption of an individual record shall not make all remaining valid data inaccessible. |
| NFR-07 | Security | The system shall operate with the minimum privileges necessary. |
| NFR-08 | Security | Data from system APIs, configuration files, and imported files shall be treated as untrusted. |
| NFR-09 | Security | Secret keys shall not be stored in source code, diagnostic logs, or unencrypted configuration files. |
| NFR-10 | Security | Well-established libraries and widely accepted algorithms shall be used for cryptographic operations. |
| NFR-11 | Security | The authenticity of updates shall be verified before installation. |
| NFR-12 | Privacy | User data shall be stored locally by default. |
| NFR-13 | Privacy | Network transmission of user data shall be disabled until the user gives explicit consent. |
| NFR-14 | Privacy | The user shall be able to disable window title collection and define rules for masking sensitive data. |
| NFR-15 | Privacy | During synchronization, data shall be transmitted over a secure channel. |
| NFR-16 | Privacy | Backups containing sensitive data shall be stored in encrypted form. |
| NFR-17 | Privacy | The documentation shall describe the data collected, where it is stored, the rules governing its transmission, and the deletion procedure. |
| NFR-18 | Compatibility | Limitations imposed by system permissions and target-platform APIs shall be documented for the user. |
| NFR-19 | Usability | Suspicious, incomplete, and verified intervals shall be visually distinguishable. |
| NFR-20 | Usability | Error messages shall describe the impact of the error and the actions available to the user. |
| NFR-21 | Maintainability | The components responsible for data collection, session calculation, storage, integrity verification, and presentation shall be separated. |
| NFR-22 | Maintainability | Public APIs and complex logic shall be documented. |
| NFR-23 | Code Quality | The code shall undergo automated formatting, static analysis, and testing. |
| NFR-24 | Testability | Time-calculation and integrity-verification logic shall be covered by automated tests in accordance with the specified coverage target. |
| NFR-25 | Testability | Platform integrations shall provide test adapters for reproducing system events. |
| NFR-26 | Security Limitations | The documentation and user interface shall not claim absolute protection against a user who has full control of the operating system. |
