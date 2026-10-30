export interface UserSummary {
  id: string;
  name: string;
}

export type ObservationType =
  | "Idle"
  | "FocusedApplication"
  | "Keyboard"
  | "MouseClick"
  | "MouseMove";

export type AdapterStatus =
  | "Available"
  | { Unavailable: { reason: string } };

export type UserActionKind =
  | "CollectorStarted"
  | "CollectorStopped"
  | "Heartbeat"
  | { IdleChanged: { idle: boolean } }
  | {
      FocusedApplicationChanged: {
        app_id: string;
        title: string | null;
      };
    }
  | {
      AdapterStatus: {
        adapter_id: string;
        observation_type: ObservationType;
        status: AdapterStatus;
      };
    };

export interface UserAction {
  id: string;
  device_id: string;
  occurred_at: string;
  kind: UserActionKind;
}
