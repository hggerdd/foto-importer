//! Arten der Aufträge aus `launch`. Die Art steuert Vorrang, Warteschlange und Pausieren;
//! ihr Name (`as_str`) erscheint unverändert im Diagnose-Log (`art=…`) und dient als
//! Schlüssel der Lese-Warteschlange. Die Namen bleiben deshalb stabil, damit
//! Log-Auswertungen weiter passen.
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum JobKind {
    /// Einzelne Nutzeränderung (Speichern, Löschen, ausdrücklicher Lauf).
    Mutation,
    Faces,
    Maintenance,
    FaceKnowledge,
    FaceAnalysis,
    MetadataBg,
    IndexNew,
    Copy,
    Picker,
    Naming,
    Undo,
    Backup,
    BackupDaily,
    BackupList,
    DngUpdates,
    RuntimeDownload,
    ModelInstall,
    Scan,
    Counts,
    Search,
    Detail,
    Suggestions,
    Report,
    Folders,
    Missing,
    PersonInsights,
    PersonDuplicates,
    RelationshipGraph,
    PersonStats,
    Timeline,
    /// Personenliste der Auswahlfelder prüfen und lesen (`people_list`).
    People,
    TimelineExport,
}

impl JobKind {
    /// Name im Diagnose-Log und Schlüssel der Lese-Warteschlange.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Mutation => "mutation",
            Self::Faces => "faces",
            Self::Maintenance => "maintenance",
            Self::FaceKnowledge => "face-knowledge",
            Self::FaceAnalysis => "face-analysis",
            Self::MetadataBg => "metadata-bg",
            Self::IndexNew => "index-new",
            Self::Copy => "copy",
            Self::Picker => "picker",
            Self::Naming => "naming",
            Self::Undo => "undo",
            Self::Backup => "backup",
            Self::BackupDaily => "backup-daily",
            Self::BackupList => "backup-list",
            Self::DngUpdates => "dngupdates",
            Self::RuntimeDownload => "runtime-download",
            Self::ModelInstall => "model-install",
            Self::Scan => "scan",
            Self::Counts => "counts",
            Self::Search => "search",
            Self::Detail => "detail",
            Self::Suggestions => "suggestions",
            Self::Report => "report",
            Self::Folders => "folders",
            Self::Missing => "missing",
            Self::PersonInsights => "person-insights",
            Self::PersonDuplicates => "person-duplicates",
            Self::RelationshipGraph => "relationship-graph",
            Self::PersonStats => "person-stats",
            Self::Timeline => "timeline",
            Self::People => "people-list",
            Self::TimelineExport => "timeline-export",
        }
    }
    /// Hintergrundarbeit mit niedriger Priorität: zählt nicht als belegt und weicht jedem
    /// Nutzerschreibauftrag.
    pub(super) fn low_priority(self) -> bool {
        matches!(self, Self::Maintenance | Self::MetadataBg | Self::IndexNew)
    }
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::JobKind;

    /// Die Log-Namen sind eindeutig und entsprechen den bisherigen Zeichenketten.
    #[test]
    fn log_names_stay_stable_and_unique() {
        let all = [
            JobKind::Mutation,
            JobKind::Faces,
            JobKind::Maintenance,
            JobKind::FaceKnowledge,
            JobKind::FaceAnalysis,
            JobKind::MetadataBg,
            JobKind::IndexNew,
            JobKind::Copy,
            JobKind::Picker,
            JobKind::Naming,
            JobKind::Undo,
            JobKind::Backup,
            JobKind::BackupDaily,
            JobKind::BackupList,
            JobKind::DngUpdates,
            JobKind::RuntimeDownload,
            JobKind::ModelInstall,
            JobKind::Scan,
            JobKind::Counts,
            JobKind::Search,
            JobKind::Detail,
            JobKind::Suggestions,
            JobKind::Report,
            JobKind::Folders,
            JobKind::Missing,
            JobKind::PersonInsights,
            JobKind::PersonDuplicates,
            JobKind::RelationshipGraph,
            JobKind::PersonStats,
            JobKind::Timeline,
            JobKind::People,
            JobKind::TimelineExport,
        ];
        let names: std::collections::BTreeSet<_> = all.iter().map(|k| k.as_str()).collect();
        assert_eq!(names.len(), all.len());
        assert_eq!(JobKind::MetadataBg.to_string(), "metadata-bg");
        assert_eq!(JobKind::DngUpdates.to_string(), "dngupdates");
        assert_eq!(
            format!("art={}", JobKind::FaceAnalysis),
            "art=face-analysis"
        );
    }
}
