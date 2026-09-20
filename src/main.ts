import { invoke } from "@tauri-apps/api/core";
import "./style.css";

type Phase = "waiting" | "running" | "done" | "error";
type SetupState = { phase: Phase; current: number; message: string; log: string; installed: boolean };

const steps = [
  ["System check", "Checking your Linux setup"],
  ["Compatibility engine", "Preparing GE-Proton 11-6"],
  ["Private Windows environment", "Creating an isolated prefix"],
  ["Web components", "Installing .NET 8 and WebView2"],
  ["Madium installer", "Completing installation in Madium window"],
  ["Finishing touches", "Verifying files and creating shortcuts"]
];

let state: SetupState = {
  phase: "waiting",
  current: 0,
  message: "Everything needed to install Madium is included.",
  log: "",
  installed: false
};
let detailsOpen = false;

function mark(index: number) {
  const complete = (state.phase === "done" || state.installed) ? true : index < state.current;
  const active = state.phase === "running" && index === state.current;
  const failed = state.phase === "error" && index === state.current;
  return `<span class="mark ${complete ? "complete" : ""} ${active ? "active" : ""} ${failed ? "failed" : ""}" aria-hidden="true">${complete ? "✓" : failed ? "!" : ""}</span>`;
}

function escapeHtml(value: string) {
  const e = document.createElement("div");
  e.textContent = value;
  return e.innerHTML;
}

function render() {
  const busy = state.phase === "running";
  const action = state.installed ? "Open Madium" : busy ? "Setting things up…" : state.phase === "error" ? "Try again" : "Install Madium";
  const appEl = document.querySelector<HTMLDivElement>("#app");
  if (!appEl) return;

  appEl.innerHTML = `
    <section class="window" aria-labelledby="title">
      <header>
        <div>
          <h1 id="title">${state.installed ? "Madium is ready" : "Let’s get Madium ready"}</h1>
          <p>MadLinux <span></span> Setup 0.1.0</p>
        </div>
        <button class="close" aria-label="Close" data-close>×</button>
      </header>
      <div class="steps" role="list" aria-label="Setup progress">
        ${steps.map((s, i) => {
          const isComplete = (state.phase === "done" || state.installed) ? true : i < state.current;
          const isActive = busy && i === state.current;
          const isPending = !isComplete && !isActive;
          return `<div class="step ${isPending ? "pending" : ""}" role="listitem">
            ${mark(i)}
            <div>
              <strong>${s[0]}</strong>
              <small>${isActive ? escapeHtml(state.message) : s[1]}</small>
            </div>
            ${isComplete ? `<em>ready</em>` : isActive ? `<em>working</em>` : ""}
          </div>`;
        }).join("")}
      </div>
      ${state.phase === "error" ? `<div class="error" role="alert"><strong>Setup needs attention</strong><span>${escapeHtml(state.message)}</span></div>` : ""}
      <div class="details ${detailsOpen ? "open" : ""}">
        <button data-details aria-expanded="${detailsOpen}">Diagnostics <span>${detailsOpen ? "−" : "+"}</span></button>
        ${detailsOpen ? `<pre>${escapeHtml(state.log || "No diagnostic messages yet.")}</pre>` : ""}
      </div>
      <footer>
        <p>${state.installed ? "Madium is installed in ~/.local/share/madlinux." : "MadLinux keeps everything in ~/.local/share/madlinux."}</p>
        <div class="actions">
          ${state.installed ? `<button class="secondary" data-reinstall>Reinstall Madium</button>` : ""}
          <button class="primary" data-action ${busy ? "disabled" : ""}>${action}</button>
        </div>
      </footer>
    </section>`;
  bind();
}

function bind() {
  document.querySelector("[data-close]")?.addEventListener("click", () => invoke("close_window"));
  document.querySelector("[data-details]")?.addEventListener("click", () => {
    detailsOpen = !detailsOpen;
    render();
  });
  document.querySelector("[data-reinstall]")?.addEventListener("click", async () => {
    state.installed = false;
    state.phase = "running";
    state.current = 0;
    state.message = "Starting setup…";
    render();
    try {
      await invoke("begin_setup");
      poll();
    } catch (error) {
      state.phase = "error";
      state.message = String(error);
      render();
    }
  });
  document.querySelector("[data-action]")?.addEventListener("click", async () => {
    if (state.installed) {
      const btn = document.querySelector<HTMLButtonElement>("[data-action]");
      if (btn) {
        btn.disabled = true;
        btn.textContent = "Opening Madium…";
      }
      try {
        await invoke("launch_madium");
        if (btn) {
          btn.textContent = "Madium Launched!";
        }
        setTimeout(() => {
          if (btn) {
            btn.disabled = false;
            btn.textContent = "Open Madium";
          }
        }, 3000);
      } catch (error) {
        state.phase = "error";
        state.message = `Failed to launch Madium: ${String(error)}`;
        render();
      }
      return;
    }
    state.phase = "running";
    state.current = 0;
    state.message = "Starting setup…";
    render();
    try {
      await invoke("begin_setup");
      poll();
    } catch (error) {
      state.phase = "error";
      state.message = String(error);
      render();
    }
  });
}

async function poll() {
  try {
    state = await invoke<SetupState>("setup_state");
    render();
    if (state.phase === "running") setTimeout(poll, 700);
  } catch (error) {
    state.phase = "error";
    state.message = String(error);
    render();
  }
}

invoke<SetupState>("setup_state")
  .then((s) => {
    state = s;
    render();
  })
  .catch(render);
