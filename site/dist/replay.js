"use strict";

const id = (name) => document.getElementById(name);

function setupCopy() {
  const button = id("copy-install");
  button.hidden = false;
  let timer;
  button.addEventListener("click", async () => {
    clearTimeout(timer);
    const command = id("install-command");
    try {
      await navigator.clipboard.writeText(command.textContent);
      button.textContent = "Copied";
      id("copy-status").textContent = "Installation commands copied.";
    } catch {
      const range = document.createRange();
      range.selectNodeContents(command);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      button.textContent = "Selected";
      id("copy-status").textContent = "Use your clipboard shortcut to copy the selected commands.";
    }
    timer = setTimeout(() => { button.textContent = "Copy commands"; }, 2400);
  });
}

function setupReplay({scenes, versions}) {
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
  const grid = id("shell-grid");
  const keyStrip = id("recorded-keys");
  const examples = id("examples");
  const replay = id("replay");
  const status = id("replay-status");
  const shells = {bash: "Bash", fish: "fish", zsh: "zsh"};
  const modes = {stock: "Shell defaults", enabled: "With COMPLETE-ME"};
  const screens = {};
  let scene;
  let timer;

  for (const [shell, name] of Object.entries(shells)) {
    const lane = document.createElement("section");
    lane.className = "shell-lane";
    lane.dataset.shell = shell;
    const heading = document.createElement("h2");
    heading.textContent = name;
    lane.append(heading);
    screens[shell] = {};

    for (const [mode, label] of Object.entries(modes)) {
      const result = document.createElement("div");
      result.className = "shell-result";
      result.dataset.mode = mode;
      const caption = document.createElement("h3");
      caption.textContent = label;
      const terminal = document.createElement("pre");
      terminal.setAttribute("aria-label", `${name}, ${label}`);
      terminal.tabIndex = 0;
      screens[shell][mode] = terminal;
      result.append(caption, terminal);
      lane.append(result);
    }
    grid.append(lane);
  }

  function render(frame) {
    for (const shell of Object.keys(shells)) {
      for (const mode of Object.keys(modes)) {
        screens[shell][mode].textContent = scene.shells[shell][mode][frame];
      }
    }
    for (const [index, key] of [...keyStrip.children].entries()) {
      key.classList.toggle("pressed", index < frame);
      if (index + 1 === frame) key.setAttribute("aria-current", "step");
      else key.removeAttribute("aria-current");
    }
    grid.dataset.frame = String(frame);
    if (frame === scene.keys.length) status.textContent = `${scene.title}. Showing results after ${scene.keys.join(", ")}.`;
  }

  function select(example) {
    clearTimeout(timer);
    scene = example;
    status.textContent = "";
    for (const button of examples.children) button.setAttribute("aria-pressed", String(button.dataset.example === scene.id));
    id("recorded-command").textContent = scene.command || "At an empty prompt";
    id("example-hint").textContent = scene.hint;
    keyStrip.replaceChildren(...scene.keys.map((key) => {
      const node = document.createElement("kbd");
      node.textContent = key;
      return node;
    }));
    render(scene.keys.length);
  }

  for (const example of scenes) {
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.example = example.id;
    button.textContent = example.title;
    button.addEventListener("click", () => select(example));
    examples.append(button);
  }

  replay.addEventListener("click", () => {
    clearTimeout(timer);
    status.textContent = "";
    if (reducedMotion.matches) {
      render(scene.keys.length);
      return;
    }
    render(0);
    function advance(frame) {
      render(frame);
      if (frame < scene.keys.length) timer = setTimeout(() => advance(frame + 1), 400);
    }
    timer = setTimeout(() => advance(1), 320);
  });

  id("recording-note").textContent = versions.bashCompletion
    ? "Recorded in real shells. The Bash baseline includes bash-completion."
    : "Recorded in real shells using their bundled completions.";
  select(scenes[0]);
  id("replay-fallback").hidden = true;
  id("replay-demo").hidden = false;
}

setupCopy();
if (window.completeMeRecordings) setupReplay(window.completeMeRecordings);
