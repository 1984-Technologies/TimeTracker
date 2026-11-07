import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { BarChart3 } from "lucide-react";
import { buildAppActivityChart, formatDuration } from "../analytics";
import type { UserAction } from "../types";

interface AppActivityChartProps {
  actions: UserAction[];
  rangeStart: string;
  rangeEnd: string;
}

export function AppActivityChart({ actions, rangeStart, rangeEnd }: AppActivityChartProps) {
  const chart = buildAppActivityChart(actions, rangeStart, rangeEnd);
  const chartWidth = Math.max(760, chart.buckets.length * 12);
  const labelInterval = Math.max(0, Math.ceil(chart.buckets.length / 8) - 1);

  return (
    <section className="mb-6 overflow-hidden rounded-2xl border border-black/[0.06] bg-white shadow-panel">
      <div className="flex flex-col gap-4 border-b border-slate-100 px-5 py-5 sm:flex-row sm:items-start sm:justify-between sm:px-6">
        <div className="flex gap-3">
          <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-emerald-50 text-accent">
            <BarChart3 className="size-5" aria-hidden="true" />
          </span>
          <div>
            <h2 className="font-semibold tracking-tight">Application activity</h2>
            <p className="mt-1 max-w-2xl text-sm leading-5 text-slate-500">
              Each bar is a 15-minute window. Its segments show the proportional time spent in each
              focused application.
            </p>
          </div>
        </div>
        <div className="shrink-0 sm:text-right">
          <p className="text-xs font-medium uppercase tracking-wider text-slate-400">Tracked app time</p>
          <p className="mt-1 text-lg font-semibold text-ink">
            {formatDuration(chart.totalActiveMilliseconds)}
          </p>
        </div>
      </div>

      {chart.series.length === 0 ? (
        <div className="grid min-h-56 place-items-center px-6 text-center text-sm text-slate-500">
          No focused-application intervals were recorded in this range.
        </div>
      ) : (
        <>
          <div className="flex flex-wrap gap-x-5 gap-y-2 px-5 pt-5 sm:px-6">
            {chart.series.map((series) => (
              <div key={series.key} className="flex items-center gap-2 text-xs text-slate-600">
                <span
                  className="size-2.5 rounded-sm"
                  style={{ backgroundColor: series.color }}
                  aria-hidden="true"
                />
                <span className="font-medium text-slate-700">{series.appId}</span>
                <span className="text-slate-400">{formatDuration(series.totalMilliseconds)}</span>
              </div>
            ))}
          </div>
          <div className="overflow-x-auto px-2 pb-3 pt-2 sm:px-4">
            <div style={{ width: `${chartWidth}px` }} className="h-64" role="img" aria-label="Focused application activity by 15-minute interval">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={chart.buckets} margin={{ top: 18, right: 12, left: 12, bottom: 4 }}>
                  <CartesianGrid vertical={false} stroke="#e8ece9" strokeDasharray="3 5" />
                  <XAxis
                    dataKey="label"
                    axisLine={false}
                    tickLine={false}
                    interval={labelInterval}
                    tick={{ fill: "#7b8784", fontSize: 11 }}
                    minTickGap={20}
                  />
                  <YAxis hide domain={[0, 100]} />
                  <Tooltip
                    cursor={{ fill: "rgba(35, 122, 99, 0.06)" }}
                    formatter={(value, name) => [`${Math.round(Number(value))}%`, name]}
                    contentStyle={{
                      border: "1px solid #e2e8e5",
                      borderRadius: "12px",
                      boxShadow: "0 12px 30px rgba(23, 32, 30, 0.08)",
                      fontSize: "12px",
                    }}
                  />
                  {chart.series.map((series) => (
                    <Bar
                      key={series.key}
                      dataKey={series.key}
                      name={series.appId}
                      stackId="activity"
                      fill={series.color}
                      maxBarSize={10}
                      isAnimationActive={false}
                    />
                  ))}
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>
        </>
      )}
    </section>
  );
}
