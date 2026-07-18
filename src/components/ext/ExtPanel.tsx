import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { OrchdDownBanner } from "../OrchdDownBanner";
import { ServersTab } from "./ServersTab";
import { ToolsBrowser } from "./ToolsBrowser";
import { ConnectorsTab } from "./ConnectorsTab";
import { SkillsTab } from "./SkillsTab";
import { InvocationLog } from "./InvocationLog";
import { ArtifactsTab } from "./ArtifactsTab";
import { strings } from "../../strings";

type TabKey = "servers" | "tools" | "connectors" | "log" | "artifacts" | "skills";

const TABS: { key: TabKey; label: string }[] = [
  { key: "servers", label: strings.ext.tabs.servers },
  { key: "tools", label: strings.ext.tabs.tools },
  { key: "connectors", label: strings.ext.tabs.connectors },
  { key: "log", label: strings.ext.tabs.log },
  { key: "artifacts", label: strings.ext.tabs.artifacts },
  { key: "skills", label: strings.ext.tabs.skills },
];

const panelStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
  color: "var(--ink)",
  background: "var(--bg)",
};

const headerStyle: CSSProperties = {
  padding: "var(--sp-3) var(--sp-4)",
  borderBottom: "1px solid var(--border)",
};

const tabBarStyle: CSSProperties = {
  display: "flex",
  gap: "var(--sp-1)",
  padding: "var(--sp-2) var(--sp-4)",
  borderBottom: "1px solid var(--border)",
};

const contentStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  padding: "var(--sp-4)",
};

/**
 * "Extensions" top-level view (S-EXT §8): the MCP servers/tools/connectors/skills/trust
 * management panel, mirrors `ProjectPanel`'s tab pattern (`TABS: {key,label}[]`, ONE tab mounted
 * at a time, `activeTab` local state) + its honest-degradation discipline (the shared
 * `<OrchdDownBanner/>` renders above the tab bar whenever `orchdDown`, matching `ProjectPanel`'s
 * placement exactly).
 *
 * All six tabs are built: Servers (`ServersTab`), Tools (`ToolsBrowser`, T8),
 * Connectors (`ConnectorsTab`, T13b), Log (`InvocationLog` — invocations + audit log +
 * the spend/rate policy-cap editor, T18), Artifacts (`ArtifactsTab`, T18) and Skills
 * (`SkillsTab`, T17 — plumbing only, no runtime consumer until S6b, see that component's own doc
 * comment).
 *
 * On mount, eagerly `refreshMcpServers()` — mirrors `ProjectPanel`'s own mount-fetch effect
 * (spec §10 "honest state, always": the server list must not silently stay `[]` just because
 * nothing else happened to refresh it yet).
 */
export function ExtPanel(): JSX.Element {
  const orchdDown = useAppStore((s) => s.orchdDown);
  const refreshMcpServers = useAppStore((s) => s.refreshMcpServers);

  const [activeTab, setActiveTab] = useState<TabKey>("servers");

  useEffect(() => {
    void refreshMcpServers();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div data-testid="ext-panel" style={panelStyle}>
      <div style={headerStyle}>
        <div style={{ fontSize: "var(--fs-lg)", fontWeight: 700, color: "var(--ink)" }}>
          {strings.ext.panelTitle}
        </div>
      </div>

      {orchdDown && <OrchdDownBanner />}

      <div role="tablist" style={tabBarStyle}>
        {TABS.map((t) => {
          const active = activeTab === t.key;
          return (
            <button
              key={t.key}
              type="button"
              role="tab"
              aria-selected={active}
              data-testid={`ext-tab-${t.key}`}
              onClick={() => setActiveTab(t.key)}
              style={{
                padding: "var(--sp-2) var(--sp-3)",
                fontSize: "var(--fs-md)",
                fontFamily: "var(--font-ui)",
                fontWeight: active ? 600 : 500,
                border: "none",
                borderBottom: active ? "2px solid var(--accent)" : "2px solid transparent",
                background: "transparent",
                color: active ? "var(--ink)" : "var(--muted)",
                cursor: "pointer",
              }}
            >
              {t.label}
            </button>
          );
        })}
      </div>

      <div style={contentStyle}>
        {activeTab === "servers" && <ServersTab />}
        {activeTab === "tools" && <ToolsBrowser />}
        {activeTab === "connectors" && <ConnectorsTab />}
        {activeTab === "log" && <InvocationLog />}
        {activeTab === "artifacts" && <ArtifactsTab />}
        {activeTab === "skills" && <SkillsTab />}
      </div>
    </div>
  );
}
