/* global window, document */
const invoke = window.__TAURI__.core.invoke;
const element = (id) => document.getElementById(id);
let initialized = false;
let previousStatus;
const messages = {
  checking: () => "Checking registration…",
  unregistered: () => "Waiting for administrator approval. Checking every 5 seconds.",
  registered: () => "Registered and authenticated. Activity collection is running.",
  connection_error: (state) => `Cannot check registration or sign in. Retrying in ${state.retry_seconds} seconds.`,
  stopping: () => "Stopping collection and sending queued events…",
  stopped: (state) => state.failed
    ? "The client stopped with an error. Choose Quit TimeTracker in the tray menu, then restart to try again."
    : "Collection stopped. Choose Quit TimeTracker in the tray menu to exit.",
};

async function refresh() {
  try {
    const state = await invoke("registration_snapshot");
    if (!initialized) {
      // SVG is generated locally in Rust from the validated enrollment URL.
      element("qr").innerHTML = state.qr_svg;
      element("device").textContent = state.device_id;
      element("link").value = state.enrollment_url;
      initialized = true;
    }
    if (state.status !== previousStatus || state.status === "connection_error") {
      const message = messages[state.status](state);
      if (element("status").textContent !== message) element("status").textContent = message;
      element("status").dataset.state = state.status;
      previousStatus = state.status;
    }
    const enrollmentVisible = ["checking", "unregistered", "connection_error"].includes(state.status);
    element("enrollment").hidden = !enrollmentVisible;
    element("intro").hidden = !enrollmentVisible;
    for (const id of ["open", "copy"]) element(id).disabled = !enrollmentVisible;
    element("retry").disabled = !["unregistered", "connection_error"].includes(state.status);
    element("retry").hidden = !enrollmentVisible;
    element("privacy").textContent = state.status === "registered"
      ? "Closing this window keeps TimeTracker running in the system tray. Choose Quit TimeTracker in the tray menu to stop collection and send queued events."
      : "Collection starts only after approval and authentication. Closing this window hides it to the system tray; use the tray menu to reopen or quit.";
  } catch {
    element("feedback").textContent = "Cannot read client status. Choose Quit TimeTracker in the tray menu, then restart.";
  }
  window.setTimeout(refresh, 500);
}

for (const [id, command, message] of [
  ["open", "open_enrollment_link", "Opened registration in your browser."],
  ["copy", "copy_enrollment_link", "Registration link copied."],
  ["retry", "retry_registration", "Checking again…"],
]) {
  element(id).addEventListener("click", async () => {
    element(id).disabled = true;
    try {
      await invoke(command);
      element("feedback").textContent = message;
    } catch (error) {
      element("feedback").textContent = String(error);
    } finally {
      element(id).disabled = false;
    }
  });
}
refresh();
