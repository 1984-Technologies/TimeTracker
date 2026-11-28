import { useEffect, useRef, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useLocation, useNavigate } from "react-router-dom";
import { ApiError, approveDevice, createUser, getDevices, getUsers, type Enrollment } from "../api";
import { parseEnrollment } from "../enrollment";
import { ErrorState } from "../components/Feedback";

interface Detector { detect(video: HTMLVideoElement): Promise<{ rawValue: string }[]> }
type DetectorConstructor = new (options: { formats: string[] }) => Detector;

export function DevicesPage() {
  const location = useLocation();
  const navigate = useNavigate();
  const expected = new URL(`${import.meta.env.BASE_URL}devices/register`, window.location.origin).href;
  const [enrollment, setEnrollment] = useState<Enrollment | null>(null);
  const [userId, setUserId] = useState("");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [scanning, setScanning] = useState(false);
  const video = useRef<HTMLVideoElement>(null);
  const camera = useRef<{ stream?: MediaStream; timer?: ReturnType<typeof setTimeout>; cancelled: boolean }>({ cancelled: true });
  const cache = useQueryClient();
  const users = useQuery({ queryKey: ["users"], queryFn: ({ signal }) => getUsers(signal) });
  const devices = useQuery({ queryKey: ["devices"], queryFn: ({ signal }) => getDevices(signal) });
  function stopCamera() {
    setScanning(false);
    camera.current.cancelled = true;
    clearTimeout(camera.current.timer);
    camera.current.stream?.getTracks().forEach((track) => track.stop());
    if (video.current) video.current.srcObject = null;
  }
  useEffect(() => () => stopCamera(), []);
  useEffect(() => {
    if (!location.hash) return;
    try { setEnrollment(parseEnrollment(window.location.href, expected)); setMessage(""); }
    catch (error) { setEnrollment(null); setMessage((error as Error).message); }
  }, [location.hash, expected]);
  function acceptLink(value: string) {
    const accepted = parseEnrollment(value, expected);
    setEnrollment(accepted);
    navigate({ pathname: new URL(expected).pathname, hash: new URLSearchParams({ ...accepted }).toString() }, { replace: true });
  }
  async function scan() {
    stopCamera();
    const run: typeof camera.current = { cancelled: false };
    camera.current = run;
    try {
      const Constructor = (window as unknown as { BarcodeDetector?: DetectorConstructor }).BarcodeDetector;
      if (!Constructor || !navigator.mediaDevices?.getUserMedia) throw new Error("Camera scanning is unavailable.");
      const detector = new Constructor({ formats: ["qr_code"] });
      const stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" }, audio: false });
      if (run.cancelled) { stream.getTracks().forEach((track) => track.stop()); return; }
      run.stream = stream;
      setScanning(true);
      video.current!.srcObject = stream;
      await video.current!.play();
      if (run.cancelled) return;
      setMessage("Point your camera at the device QR code.");
      const tick = async () => {
        if (run.cancelled) return;
        try {
          const codes = await detector.detect(video.current!);
          if (run.cancelled) return;
          if (codes.length) {
            acceptLink(codes[0].rawValue);
            setMessage("Link scanned. Select a user, then approve this device.");
            stopCamera(); return;
          }
          run.timer = setTimeout(tick, 300);
        } catch (error) { if (!run.cancelled) { stopCamera(); setMessage(`${(error as Error).message} Paste the registration link instead.`); } }
      };
      void tick();
    } catch (error) {
      if (!run.cancelled) { stopCamera(); setMessage(`${(error as Error).message} You can always paste the link below.`); }
    }
  }
  function paste(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); stopCamera();
    const value = String(new FormData(event.currentTarget).get("link"));
    try { acceptLink(value.trim()); setMessage("Link loaded. Select a user before approval."); }
    catch (error) { setEnrollment(null); setMessage((error as Error).message); }
  }
  async function approve() {
    if (!enrollment || !userId) return;
    setBusy(true); setMessage("");
    try {
      await approveDevice(enrollment, userId);
      await cache.invalidateQueries({ queryKey: ["devices"] });
      setMessage("Device approved. Its client can now authenticate and start collection.");
    } catch (error) {
      setMessage(error instanceof ApiError && error.status === 409 ? "Approval conflict: this device already has another owner or different credentials." : (error as Error).message);
    } finally { setBusy(false); }
  }
  async function newUser(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); const form = event.currentTarget; setBusy(true);
    try { const user = await createUser(String(new FormData(form).get("name"))); await cache.invalidateQueries({ queryKey: ["users"] }); setUserId(user.id); form.reset(); }
    catch (error) { setMessage((error as Error).message); }
    finally { setBusy(false); }
  }
  return <section className="space-y-6">
    <h1 className="text-3xl font-semibold">Devices</h1>
    <div className="auth-panel space-y-4">
      <h2 className="text-xl font-semibold">Register a device</h2>
      <p>Scan, open, or paste the link shown in the desktop client. Approval always requires selecting a user.</p>
      <div className="flex gap-3"><button className="auth-button" onClick={() => void scan()}>Scan QR code</button><button className="auth-secondary" onClick={stopCamera}>Stop camera</button></div>
      <video hidden={!scanning} ref={video} autoPlay playsInline muted className="max-h-60 w-full rounded-xl bg-slate-100" aria-label="QR camera preview" />
      <form onSubmit={paste} className="space-y-3"><label className="block">Registration link<textarea name="link" className="auth-input" rows={3} required /></label><button className="auth-secondary">Load link</button></form>
      {enrollment && <div className="rounded-xl bg-emerald-50 p-4"><p>Device ID</p><p className="break-all font-mono text-sm">{enrollment.device_id}</p><p className="mt-2 break-all font-mono text-xs">Credential hash: {enrollment.credential_hash}</p></div>}
      <label className="block">Assign to user<select className="auth-input" value={userId} onChange={(event) => setUserId(event.target.value)}><option value="">Select a user</option>{users.data?.map((user) => <option key={user.id} value={user.id}>{user.name}</option>)}</select></label>
      <form onSubmit={(event) => void newUser(event)} className="flex flex-wrap items-end gap-3"><label className="grow">Or create a user<input name="name" className="auth-input" required maxLength={200} /></label><button className="auth-secondary" disabled={busy}>Create user</button></form>
      <button className="auth-button" disabled={busy || !enrollment || !userId} onClick={() => void approve()}>Approve device</button>
      {message && <p role="status" aria-live="polite">{message}</p>}
      {users.isError && <ErrorState error={users.error} />}
    </div>
    <div className="auth-panel"><h2 className="mb-4 text-xl font-semibold">Approved devices</h2>
      {devices.isPending ? <p>Loading devices…</p> : devices.isError ? <ErrorState error={devices.error} /> : devices.data.length ? <ul className="space-y-3">{devices.data.map((device) => <li key={device.device_id}><p className="break-all font-mono text-sm">{device.device_id}</p><p className="text-slate-500">{users.data?.find((user) => user.id === device.user_id)?.name ?? device.user_id}</p></li>)}</ul> : <p>No devices approved yet.</p>}
    </div>
  </section>;
}
