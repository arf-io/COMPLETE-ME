# COMPLETE-ME browser demo

The landing page compares recorded output from Bash, fish, and zsh. Each shell shows the
same keystrokes with its default completions and with COMPLETE-ME enabled. Select an example
to see the result immediately or use **Replay keys** to see when each Tab was pressed.

## Preview

Serve the static files from the repository root:

    python3 -m http.server 8766 --directory site/dist

Open http://localhost:8766. Deploy the `site/dist` directory to host the site.

## Refresh the recordings

Install Bash, fish, zsh, tmux, fzf, and a current `complete-me` binary on PATH. Activate a
Python virtual environment, then run:

    python3 -m pip install pyte==0.8.2
    python3 site/demo/record.py

The recorder uses real PTYs, temporary shell profiles, a sample Python command, and a separate
tmux server. It captures versions in `site/dist/recordings.js`. Bash uses bash-completion
when installed in a supported system location. Review the generated diff before committing it.

## Browser tests

With the static server running:

    npm install --prefix target/site-qa playwright-core@1.65.1
    node target/site-qa/node_modules/playwright-core/cli.js install chromium
    node site/demo/browser-test.mjs

Set `CM_DEMO_CHROME` to use an installed Chromium-based browser, or `CM_DEMO_URL` for a
different preview address.
