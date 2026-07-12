import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  Executable,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  // The language server launches an external executable (pyrst.server.path). To
  // avoid workspace-controlled code execution, only start it when the workspace
  // is TRUSTED. Syntax highlighting comes from the contributed grammar and works
  // regardless. If the user grants trust later, start the server then.
  if (vscode.workspace.isTrusted) {
    await startServer(context);
  } else {
    context.subscriptions.push(
      vscode.workspace.onDidGrantWorkspaceTrust(() => {
        void startServer(context);
      }),
    );
  }
}

async function startServer(context: vscode.ExtensionContext): Promise<void> {
  if (client) {
    return; // already started
  }
  const config = vscode.workspace.getConfiguration('pyrst');
  const serverPath: string = config.get<string>('server.path', 'pyrst');

  // (card 587a9dcb, AC3 — defense-in-depth) Launch the server with its working
  // directory set to the (first) workspace folder. The real fix is server-side
  // file-anchored env discovery (AC1): the resolver now walks up from the edited
  // file's directory to find a `.pyrstenv/`, so the editor's process CWD no longer
  // has to be right. But pinning the cwd here makes the common single-folder
  // workspace robust regardless, and gives the server a sane CWD for its fallback
  // walk. `undefined` when no folder is open (a single loose file), which leaves
  // the cwd at VS Code's default — unchanged from before.
  const workspaceCwd: string | undefined =
    vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;

  // The pyrst binary is an external native executable that speaks LSP over stdio.
  // We do NOT use TransportKind.ipc (Node.js IPC) — we use TransportKind.stdio
  // to talk to the Rust binary directly over stdin/stdout.
  const run: Executable = {
    command: serverPath,
    args: ['lsp'],
    transport: TransportKind.stdio,
    options: workspaceCwd ? { cwd: workspaceCwd } : undefined,
  };

  const debug: Executable = {
    command: serverPath,
    args: ['lsp'],
    transport: TransportKind.stdio,
    // RUST_LOG can be set in the environment to enable server-side tracing; cwd is
    // pinned to the workspace folder for the same reason as `run` above.
    options: {
      cwd: workspaceCwd,
      env: {
        ...process.env,
        RUST_LOG: 'debug',
      },
    },
  };

  const serverOptions: ServerOptions = { run, debug };

  const clientOptions: LanguageClientOptions = {
    // Activate the client for .pyrs files opened as local files.
    documentSelector: [{ scheme: 'file', language: 'pyrst' }],
    // Relay the pyrst.trace.server setting to the LSP tracing infrastructure.
    traceOutputChannel: vscode.window.createOutputChannel('Pyrst Language Server Trace'),
  };

  client = new LanguageClient(
    'pyrst',
    'Pyrst Language Server',
    serverOptions,
    clientOptions,
  );

  // start() is async in vscode-languageclient v9+ and resolves once the
  // initialize/initialized handshake has completed.
  await client.start();

  context.subscriptions.push(client);
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
  }
}
