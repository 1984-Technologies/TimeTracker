import {
  Activity,
  AppWindow,
  Keyboard,
  MousePointer2,
  MousePointerClick,
  type LucideIcon,
} from "lucide-react";
import { deriveSensorStatuses } from "../analytics";
import { formatDateTime } from "../lib";
import type { ObservationType, UserAction } from "../types";

const sensorPresentation: Record<ObservationType, { label: string; icon: LucideIcon }> = {
  Idle: { label: "Idle detection", icon: Activity },
  FocusedApplication: { label: "Focused application", icon: AppWindow },
  Keyboard: { label: "Keyboard", icon: Keyboard },
  MouseClick: { label: "Mouse clicks", icon: MousePointerClick },
  MouseMove: { label: "Mouse movement", icon: MousePointer2 },
};

export function SensorStatusGrid({ actions }: { actions: UserAction[] }) {
  const sensors = deriveSensorStatuses(actions);

  return (
    <section className="relative z-20 mb-4 flex min-h-16 flex-col gap-3 rounded-2xl border border-black/[0.06] bg-white px-4 py-3 shadow-sm sm:flex-row sm:items-center">
      <div className="shrink-0">
        <div className="flex items-center gap-2">
          <h2 className="text-sm font-semibold tracking-tight">Sensor health</h2>
          <span className="rounded-full bg-slate-100 px-2 py-0.5 text-[10px] font-semibold text-slate-500">
            {sensors.length}
          </span>
        </div>
        <p className="mt-0.5 text-xs text-slate-400">Latest reported state</p>
      </div>

      {sensors.length === 0 ? (
        <p className="text-sm text-slate-500 sm:ml-auto">No sensor status events in this range.</p>
      ) : (
        <div className="flex flex-wrap gap-2 sm:ml-auto">
          {sensors.map((sensor, index) => {
            const presentation = sensorPresentation[sensor.observationType];
            const Icon = presentation.icon;
            const tooltipId = `sensor-${sensor.observationType}`;
            const tooltipAlignment =
              index === 0
                ? "left-0"
                : index === sensors.length - 1
                  ? "right-0"
                  : "left-1/2 -translate-x-1/2";

            return (
              <button
                key={sensor.observationType}
                type="button"
                aria-describedby={tooltipId}
                aria-label={`${presentation.label}: ${sensor.available ? "online" : "offline"}`}
                className={`group/sensor relative grid size-10 place-items-center rounded-xl border outline-none transition hover:-translate-y-0.5 focus-visible:ring-2 focus-visible:ring-accent/30 ${
                  sensor.available
                    ? "border-emerald-100 bg-emerald-50 text-emerald-700"
                    : "border-red-100 bg-red-50 text-red-700"
                }`}
              >
                <Icon className="size-4.5" aria-hidden="true" />
                <span
                  className={`absolute right-1 top-1 size-2 rounded-full border-2 border-white ${
                    sensor.available ? "bg-emerald-500" : "bg-red-500"
                  }`}
                  aria-hidden="true"
                />
                <span
                  id={tooltipId}
                  role="tooltip"
                  className={`pointer-events-none absolute top-full z-40 mt-2 w-64 rounded-xl bg-ink p-3 text-left text-white opacity-0 shadow-xl transition-opacity group-hover/sensor:opacity-100 group-focus/sensor:opacity-100 ${tooltipAlignment}`}
                >
                  <span className="flex items-center justify-between gap-3">
                    <span className="text-xs font-semibold">{presentation.label}</span>
                    <span
                      className={`text-[10px] font-semibold uppercase tracking-wider ${
                        sensor.available ? "text-emerald-300" : "text-red-300"
                      }`}
                    >
                      {sensor.available ? "Online" : "Offline"}
                    </span>
                  </span>
                  <span className="mt-2 block font-mono text-[10px] text-slate-300">
                    {sensor.adapterId}
                  </span>
                  {sensor.reason && (
                    <span className="mt-2 block text-xs leading-5 text-red-200">{sensor.reason}</span>
                  )}
                  <span className="mt-2 block text-[10px] text-slate-400">
                    Updated {formatDateTime(sensor.occurredAt)}
                  </span>
                </span>
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}
