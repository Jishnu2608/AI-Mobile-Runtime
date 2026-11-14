/**
 * The product's real relationships, drawn as a diagram (not a screenshot, not a fake UI):
 * coding agent ⇄ umr MCP / CLI ⇄ Runtime Core ⇄ Android emulator, the live preview window, and
 * problems flowing back to the agent with file and line. Labels are real tool and command names.
 * Animated by the Hero component (data-* hooks); complete and readable without script.
 */
export function HeroDiagram() {
  return (
    <svg
      viewBox="0 0 640 448"
      role="img"
      aria-labelledby="hero-diagram-title hero-diagram-desc"
      className="h-auto w-full"
      data-diagram
    >
      <title id="hero-diagram-title">How the runtime connects your coding agent and the emulator</title>
      <desc id="hero-diagram-desc">
        The coding agent calls the umr MCP tools. The Runtime Core builds and runs the app on the Android emulator, streams the
        screen to the preview window, and returns problems with file and line to the agent.
      </desc>
      <defs>
        <pattern id="hd-grid" width="20" height="20" patternUnits="userSpaceOnUse">
          <path d="M20 0H0V20" fill="none" stroke="var(--line)" strokeWidth="1" />
        </pattern>
        <marker id="hd-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
          <path d="M0 0 8 4 0 8z" fill="var(--accent)" />
        </marker>
      </defs>

      <rect x="0.5" y="0.5" width="639" height="447" rx="18" fill="url(#hd-grid)" opacity="0.55" />

      {/* connectors (drawn on enter) */}
      <g fill="none" stroke="var(--accent)" strokeWidth="1.5" data-lines>
        <path id="hd-l1" d="M206 150 H262" markerEnd="url(#hd-arrow)" />
        <path id="hd-l2" d="M262 196 H206" markerEnd="url(#hd-arrow)" />
        <path id="hd-l3" d="M440 172 H486" markerEnd="url(#hd-arrow)" />
        <path id="hd-l4" d="M350 266 V328" markerEnd="url(#hd-arrow)" />
        <path id="hd-l5" d="M262 246 C236 246 236 318 214 318" markerEnd="url(#hd-arrow)" strokeDasharray="4 5" />
        <path id="hd-l6" d="M112 300 V266" markerEnd="url(#hd-arrow)" />
      </g>

      {/* pulses travelling along the connectors (positioned by script) */}
      <g data-pulses aria-hidden>
        <circle r="4" fill="var(--lime)" stroke="var(--forest)" strokeWidth="1" data-pulse="hd-l1" opacity="0" />
        <circle r="4" fill="var(--lime)" stroke="var(--forest)" strokeWidth="1" data-pulse="hd-l3" opacity="0" />
        <circle r="4" fill="var(--lime)" stroke="var(--forest)" strokeWidth="1" data-pulse="hd-l5" opacity="0" />
      </g>

      {/* coding agent */}
      <g data-node>
        <rect x="24" y="96" width="182" height="166" rx="12" fill="var(--surface)" stroke="var(--line-strong)" />
        <rect x="24" y="96" width="182" height="30" rx="12" fill="var(--surface-sunken)" />
        <rect x="24" y="114" width="182" height="12" fill="var(--surface-sunken)" />
        <text x="40" y="116" fontSize="12" fontWeight="600" fill="var(--ink)">Coding agent</text>
        <text x="40" y="150" fontSize="11" fontFamily="var(--ff-mono)" fill="var(--ink-2)">Claude Code · Codex</text>
        <text x="40" y="178" fontSize="11" fontFamily="var(--ff-mono)" fill="var(--ink)">runtime_status</text>
        <text x="40" y="198" fontSize="11" fontFamily="var(--ff-mono)" fill="var(--ink)">get_diagnostic</text>
        <text x="40" y="218" fontSize="11" fontFamily="var(--ff-mono)" fill="var(--ink)">screenshot · tap</text>
        <text x="40" y="238" fontSize="11" fontFamily="var(--ff-mono)" fill="var(--ink)">reload</text>
      </g>

      {/* runtime core */}
      <g data-node>
        <rect x="262" y="96" width="178" height="170" rx="12" fill="var(--forest)" />
        <text x="280" y="124" fontSize="12" fontWeight="600" fill="#edf5ed">Runtime Core</text>
        <text x="280" y="142" fontSize="10.5" fontFamily="var(--ff-mono)" fill="#b1c5b4">umr mcp · umr</text>
        {[
          ["toolchain", 168],
          ["device + session", 190],
          ["build + reload", 212],
          ["problems + logs", 234],
        ].map(([t, y]) => (
          <g key={t as string}>
            <rect x="280" y={(y as number) - 13} width="142" height="18" rx="4" fill="#216b50" />
            <text x="290" y={(y as number) + 0.5} fontSize="10.5" fill="#d1e8d7">{t}</text>
          </g>
        ))}
      </g>

      {/* emulator: a phone outline with the user's app, not a mock of a particular app */}
      <g data-node>
        <rect x="486" y="40" width="130" height="276" rx="22" fill="var(--surface)" stroke="var(--line-strong)" strokeWidth="1.5" />
        <rect x="498" y="66" width="106" height="224" rx="6" fill="var(--surface-sunken)" />
        <rect x="533" y="50" width="36" height="6" rx="3" fill="var(--line-strong)" />
        <rect x="510" y="80" width="62" height="9" rx="3" fill="var(--sage)" />
        <rect x="510" y="98" width="82" height="6" rx="3" fill="var(--line-strong)" />
        <rect x="510" y="110" width="70" height="6" rx="3" fill="var(--line-strong)" />
        <rect x="510" y="128" width="82" height="58" rx="6" fill="var(--mint)" />
        <rect x="510" y="196" width="82" height="6" rx="3" fill="var(--line-strong)" />
        <rect x="510" y="208" width="58" height="6" rx="3" fill="var(--line-strong)" />
        <circle cx="580" cy="268" r="11" fill="var(--lime)" stroke="var(--forest)" />
        <text x="551" y="334" fontSize="12" fontWeight="600" fill="var(--ink)" textAnchor="middle">Android emulator</text>
        <text x="551" y="350" fontSize="10.5" fontFamily="var(--ff-mono)" fill="var(--ink-2)" textAnchor="middle">your app, live</text>
      </g>

      {/* preview window */}
      <g data-node>
        <rect x="262" y="328" width="178" height="96" rx="10" fill="var(--surface)" stroke="var(--line-strong)" />
        <text x="278" y="352" fontSize="12" fontWeight="600" fill="var(--ink)">Preview window</text>
        <text x="278" y="372" fontSize="10.5" fontFamily="var(--ff-mono)" fill="var(--ink-2)">docked beside the agent</text>
        <text x="278" y="390" fontSize="10.5" fontFamily="var(--ff-mono)" fill="var(--ink-2)">mouse · keyboard · rotate</text>
        <text x="278" y="408" fontSize="10.5" fontFamily="var(--ff-mono)" fill="var(--ink-2)">problems · logs</text>
      </g>

      {/* labels on the connectors */}
      <g fontSize="10" fontFamily="var(--ff-mono)" fill="var(--ink-2)">
        <text x="234" y="142" textAnchor="middle">MCP</text>
        <text x="463" y="164" textAnchor="middle">adb</text>
        <text x="362" y="304">frames</text>
      </g>
      {/* a problem as the agent receives it: category, origin, file and line */}
      <g data-node>
        <rect x="24" y="300" width="190" height="62" rx="10" fill="var(--lime-pale)" stroke="var(--lime-deep)" />
        <text x="38" y="322" fontSize="11" fontWeight="600" fill="var(--forest)">Problem · runtime · your code</text>
        <text x="38" y="344" fontSize="11" fontFamily="var(--ff-mono)" fill="var(--forest)">lib/main.dart:67</text>
      </g>
      <text x="234" y="300" fontSize="10" fontFamily="var(--ff-mono)" fill="var(--ink-2)" textAnchor="middle">diagnostics</text>
    </svg>
  );
}
