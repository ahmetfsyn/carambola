import React, { useEffect, useState } from "react";
import { ExternalLink } from "lucide-react";
import { getInstalledVersion } from "../../lib/appVersion";

type VersionState = { state: "loading" } | { state: "available"; version: string } | { state: "unavailable" };

export const AboutSection: React.FC = () => {
  const [version, setVersion] = useState<VersionState>({ state: "loading" });

  useEffect(() => {
    let active = true;
    getInstalledVersion()
      .then((currentVersion) => {
        if (active) setVersion({ state: "available", version: currentVersion });
      })
      .catch(() => {
        if (active) setVersion({ state: "unavailable" });
      });
    return () => {
      active = false;
    };
  }, []);

  return (
    <section className="mt-3 p-3 rounded-xl bg-slate-950/70 border border-slate-800">
      <h3 className="text-xs font-semibold text-slate-200">About</h3>
      <div className="mt-2 flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="text-xs font-semibold text-white">Carambola</div>
          <p className="mt-1 text-[11px] text-slate-400">
            Lightweight cross-platform screenshot and annotation application.
          </p>
        </div>
        <div className="shrink-0 text-right">
          <div className="text-[10px] uppercase tracking-wide text-slate-500">Current version</div>
          <div
            className={`mt-0.5 font-mono text-xs ${version.state === "unavailable" ? "text-slate-500" : "text-slate-200"}`}
            aria-live="polite"
          >
            {version.state === "available" ? version.version : version.state === "loading" ? "Loading…" : "Unavailable"}
          </div>
        </div>
      </div>
      <a
        href="https://github.com/ahmetfsyn/carambola"
        target="_blank"
        rel="noreferrer"
        className="mt-2 inline-flex items-center gap-1 text-[11px] text-carambola-cyan hover:text-cyan-300 transition-colors"
      >
        GitHub repository <ExternalLink size={11} />
      </a>
    </section>
  );
};
