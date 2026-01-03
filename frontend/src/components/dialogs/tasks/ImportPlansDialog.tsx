import { useState, useCallback } from 'react';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import { defineModal } from '@/lib/modals';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Loader } from '@/components/ui/loader';
import { Badge } from '@/components/ui/badge';
import { FolderOpen, AlertCircle, CheckCircle2, Eye, ChevronDown, ChevronRight, FileText } from 'lucide-react';
import { useImportPlans } from '@/hooks';
import { cn } from '@/lib/utils';
import { plansApi } from '@/lib/api';
import type { PlanMetadata, PlanPhaseDetail, PlanPhaseSelection } from '@/lib/api';
import WYSIWYGEditor from '@/components/ui/wysiwyg';

export interface ImportPlansDialogProps {
  projectId: string;
}

function getStatusBadgeVariant(status: string) {
  switch (status.toLowerCase()) {
    case 'in-progress':
    case 'inprogress':
      return 'secondary';
    case 'completed':
    case 'done':
      return 'default';
    case 'cancelled':
    case 'canceled':
      return 'destructive';
    default:
      return 'outline';
  }
}

// Preview modal for markdown content
function PreviewModal({
  isOpen,
  onClose,
  content,
  fileName,
  isLoading,
}: {
  isOpen: boolean;
  onClose: () => void;
  content: string;
  fileName: string;
  isLoading: boolean;
}) {
  if (!isOpen) return null;

  return (
    <Dialog open={isOpen} onOpenChange={onClose}>
      <DialogContent className="sm:max-w-[800px] max-h-[85vh] flex flex-col">
        <DialogHeader>
          <div className="flex items-center gap-2">
            <FileText className="h-5 w-5" />
            <DialogTitle>{fileName}</DialogTitle>
          </div>
        </DialogHeader>
        <div className="flex-1 min-h-0 overflow-y-auto border rounded-md p-4 bg-muted/30">
          {isLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader message="Loading content..." size={24} />
            </div>
          ) : (
            <WYSIWYGEditor value={content} disabled className="min-h-[200px]" />
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function PhaseRow({
  phase,
  checked,
  onCheckedChange,
  onPreview,
}: {
  phase: PlanPhaseDetail;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  onPreview: (filePath: string) => void;
}) {
  return (
    <div
      className={cn(
        'flex items-center gap-2 py-1.5 px-2 text-sm border-l-2 ml-4 cursor-pointer transition-colors',
        checked ? 'border-primary bg-primary/5' : 'border-muted hover:bg-muted/30'
      )}
      onClick={() => onCheckedChange(!checked)}
    >
      <Checkbox
        checked={checked}
        onCheckedChange={(e) => {
          e && onCheckedChange(!!e);
        }}
        className="shrink-0"
      />
      <Badge variant={getStatusBadgeVariant(phase.status)} className="text-xs shrink-0">
        {phase.status}
      </Badge>
      <span className="truncate flex-1">
        Phase {phase.phase}: {phase.name}
      </span>
      <Button
        variant="ghost"
        size="sm"
        className="h-6 w-6 p-0 shrink-0"
        onClick={(e) => {
          e.stopPropagation();
          onPreview(phase.file);
        }}
        title="Preview phase content"
      >
        <Eye className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}

function PlanCheckboxRow({
  plan,
  selectedPhases,
  onPhaseToggle,
  onSelectAllPhases,
  onPreview,
}: {
  plan: PlanMetadata;
  selectedPhases: Set<number>;
  onPhaseToggle: (planId: string, phaseNum: number, checked: boolean) => void;
  onSelectAllPhases: (planId: string, selectAll: boolean) => void;
  onPreview: (filePath: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const title = plan.title || plan.name;
  const truncatedDesc = plan.description
    ? plan.description.length > 100
      ? plan.description.substring(0, 100) + '...'
      : plan.description
    : null;
  const hasPhases = plan.phaseDetails && plan.phaseDetails.length > 0;
  const totalPhases = plan.phaseDetails?.length || 0;
  const allPhasesSelected = hasPhases && selectedPhases.size === totalPhases;
  const somePhasesSelected = selectedPhases.size > 0;

  const handlePlanToggle = () => {
    if (allPhasesSelected) {
      // Deselect all phases
      onSelectAllPhases(plan.id, false);
    } else {
      // Select all phases
      onSelectAllPhases(plan.id, true);
    }
  };

  // Determine check state for plan-level checkbox
  const isIndeterminate = somePhasesSelected && !allPhasesSelected;

  return (
    <div className="space-y-1">
      <div
        className={cn(
          'flex items-start gap-3 p-3 rounded-md border cursor-pointer transition-colors',
          somePhasesSelected
            ? 'border-primary bg-primary/5'
            : 'border-border hover:bg-muted/50'
        )}
        onClick={handlePlanToggle}
      >
        <Checkbox
          checked={allPhasesSelected}
          ref={(el) => {
            if (el) {
              (el as HTMLButtonElement & { indeterminate?: boolean }).indeterminate = isIndeterminate;
            }
          }}
          onCheckedChange={() => handlePlanToggle()}
          className="mt-0.5"
        />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            {hasPhases && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setExpanded(!expanded);
                }}
                className="p-0.5 hover:bg-muted rounded"
              >
                {expanded ? (
                  <ChevronDown className="h-4 w-4" />
                ) : (
                  <ChevronRight className="h-4 w-4" />
                )}
              </button>
            )}
            <span className="font-medium truncate">{title}</span>
            <Badge variant={getStatusBadgeVariant(plan.status)} className="text-xs">
              {plan.status}
            </Badge>
            <Button
              variant="ghost"
              size="sm"
              className="h-6 w-6 p-0 ml-auto shrink-0"
              onClick={(e) => {
                e.stopPropagation();
                onPreview(plan.path);
              }}
              title="Preview plan.md"
            >
              <Eye className="h-3.5 w-3.5" />
            </Button>
          </div>
          {truncatedDesc && (
            <p className="text-sm text-muted-foreground mt-1">{truncatedDesc}</p>
          )}
          <div className="flex items-center gap-3 mt-1 text-xs text-muted-foreground">
            <span>
              {hasPhases
                ? selectedPhases.size > 0
                  ? `${selectedPhases.size}/${totalPhases} phases selected`
                  : `${totalPhases} phases`
                : '1 task'}
            </span>
            {plan.priority && <span>Priority: {plan.priority}</span>}
          </div>
        </div>
      </div>
      {expanded && hasPhases && (
        <div className="space-y-0.5 pb-2">
          {plan.phaseDetails.map((phase) => (
            <PhaseRow
              key={`${plan.id}-${phase.phase}`}
              phase={phase}
              checked={selectedPhases.has(phase.phase)}
              onCheckedChange={(checked) =>
                onPhaseToggle(plan.id, phase.phase, checked)
              }
              onPreview={onPreview}
            />
          ))}
        </div>
      )}
    </div>
  );
}

const ImportPlansDialogImpl = NiceModal.create<ImportPlansDialogProps>(
  (props) => {
    const { projectId } = props;
    const modal = useModal();
    const { listPlans, importPlans } = useImportPlans(projectId);
    // Map of planId -> Set of selected phase numbers
    const [selections, setSelections] = useState<Map<string, Set<number>>>(
      new Map()
    );
    const [importResult, setImportResult] = useState<{
      count: number;
      errors: string[];
    } | null>(null);

    // Preview state
    const [previewOpen, setPreviewOpen] = useState(false);
    const [previewContent, setPreviewContent] = useState('');
    const [previewFileName, setPreviewFileName] = useState('');
    const [previewLoading, setPreviewLoading] = useState(false);

    const plans = listPlans.data || [];
    const isLoading = listPlans.isLoading;
    const isImporting = importPlans.isPending;

    // Get selected phases for a plan
    const getSelectedPhases = useCallback(
      (planId: string): Set<number> => {
        return selections.get(planId) || new Set();
      },
      [selections]
    );

    // Toggle individual phase selection
    const handlePhaseToggle = useCallback(
      (planId: string, phaseNum: number, checked: boolean) => {
        setSelections((prev) => {
          const next = new Map(prev);
          const phases = new Set(next.get(planId) || []);
          if (checked) {
            phases.add(phaseNum);
          } else {
            phases.delete(phaseNum);
          }
          if (phases.size === 0) {
            next.delete(planId);
          } else {
            next.set(planId, phases);
          }
          return next;
        });
      },
      []
    );

    // Select/deselect all phases for a plan
    const handleSelectAllPhases = useCallback(
      (planId: string, selectAll: boolean) => {
        const plan = plans.find((p) => p.id === planId);
        if (!plan) return;

        setSelections((prev) => {
          const next = new Map(prev);
          if (selectAll && plan.phaseDetails && plan.phaseDetails.length > 0) {
            const allPhases = new Set(plan.phaseDetails.map((p) => p.phase));
            next.set(planId, allPhases);
          } else {
            next.delete(planId);
          }
          return next;
        });
      },
      [plans]
    );

    // Select/deselect all plans
    const handleSelectAll = useCallback(() => {
      const totalSelected = Array.from(selections.values()).reduce(
        (sum, phases) => sum + phases.size,
        0
      );
      const totalPhases = plans.reduce(
        (sum, p) => sum + (p.phaseDetails?.length || 1),
        0
      );

      if (totalSelected === totalPhases) {
        // Deselect all
        setSelections(new Map());
      } else {
        // Select all phases of all plans
        const allSelections = new Map<string, Set<number>>();
        plans.forEach((plan) => {
          if (plan.phaseDetails && plan.phaseDetails.length > 0) {
            allSelections.set(
              plan.id,
              new Set(plan.phaseDetails.map((p) => p.phase))
            );
          } else {
            // For plans without phases, use phase 0 as placeholder
            allSelections.set(plan.id, new Set([0]));
          }
        });
        setSelections(allSelections);
      }
    }, [plans, selections]);

    // Count total selected phases
    const totalSelectedPhases = Array.from(selections.values()).reduce(
      (sum, phases) => sum + phases.size,
      0
    );

    const handlePreview = useCallback(
      async (filePath: string) => {
        setPreviewOpen(true);
        setPreviewLoading(true);
        setPreviewContent('');
        setPreviewFileName('Loading...');

        try {
          const result = await plansApi.readFile(projectId, filePath);
          setPreviewContent(result.content);
          setPreviewFileName(result.file_name);
        } catch (err) {
          setPreviewContent('Failed to load file content.');
          setPreviewFileName('Error');
          console.error('Failed to load file:', err);
        } finally {
          setPreviewLoading(false);
        }
      },
      [projectId]
    );

    const handleImport = useCallback(async () => {
      if (selections.size === 0) return;

      // Build selections array for the API
      const selectionsArray: PlanPhaseSelection[] = Array.from(
        selections.entries()
      ).map(([planId, phases]) => ({
        plan_id: planId,
        phases: Array.from(phases).filter((p) => p !== 0), // Filter out placeholder phase 0
      }));

      try {
        const result = await importPlans.mutateAsync({
          project_id: projectId,
          plan_ids: null,
          selections: selectionsArray,
        });
        setImportResult({
          count: result.imported_count,
          errors: result.errors,
        });

        // Close dialog after short delay if no errors
        if (result.errors.length === 0) {
          setTimeout(() => modal.remove(), 1500);
        }
      } catch (err) {
        console.error('Failed to import plans:', err);
      }
    }, [selections, projectId, importPlans, modal]);

    const handleClose = () => {
      modal.remove();
    };

    return (
      <>
        <Dialog open={modal.visible} onOpenChange={handleClose}>
          <DialogContent className="sm:max-w-[600px] max-h-[80vh] flex flex-col">
            <DialogHeader>
              <div className="flex items-center gap-2">
                <FolderOpen className="h-5 w-5" />
                <DialogTitle>Import Plans as Tasks</DialogTitle>
              </div>
              <DialogDescription>
                Select plans or individual phases to import as tasks.
              </DialogDescription>
            </DialogHeader>

            <div className="flex-1 min-h-0 overflow-y-auto space-y-2 py-2">
              {isLoading && (
                <div className="flex items-center justify-center py-8">
                  <Loader message="Loading plans..." size={24} />
                </div>
              )}

              {!isLoading && plans.length === 0 && (
                <div className="text-center py-8 text-muted-foreground">
                  <FolderOpen className="h-12 w-12 mx-auto mb-2 opacity-50" />
                  <p>No plans found in the plans/ folder.</p>
                </div>
              )}

              {!isLoading && plans.length > 0 && !importResult && (
                <>
                  <div className="flex items-center justify-between pb-2">
                    <span className="text-sm text-muted-foreground">
                      {plans.length} plan{plans.length !== 1 ? 's' : ''} found
                    </span>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={handleSelectAll}
                      className="text-xs"
                    >
                      {totalSelectedPhases > 0 ? 'Deselect All' : 'Select All'}
                    </Button>
                  </div>
                  {plans.map((plan) => (
                    <PlanCheckboxRow
                      key={plan.id}
                      plan={plan}
                      selectedPhases={getSelectedPhases(plan.id)}
                      onPhaseToggle={handlePhaseToggle}
                      onSelectAllPhases={handleSelectAllPhases}
                      onPreview={handlePreview}
                    />
                  ))}
                </>
              )}

              {importResult && (
                <div className="text-center py-8">
                  {importResult.errors.length === 0 ? (
                    <>
                      <CheckCircle2 className="h-12 w-12 mx-auto mb-2 text-green-500" />
                      <p className="font-medium">
                        Successfully imported {importResult.count} task
                        {importResult.count !== 1 ? 's' : ''}!
                      </p>
                    </>
                  ) : (
                    <>
                      <AlertCircle className="h-12 w-12 mx-auto mb-2 text-yellow-500" />
                      <p className="font-medium">
                        Imported {importResult.count} task
                        {importResult.count !== 1 ? 's' : ''} with{' '}
                        {importResult.errors.length} error
                        {importResult.errors.length !== 1 ? 's' : ''}
                      </p>
                      <ul className="text-sm text-destructive mt-2 text-left max-h-40 overflow-y-auto">
                        {importResult.errors.map((err, i) => (
                          <li key={i}>{err}</li>
                        ))}
                      </ul>
                    </>
                  )}
                </div>
              )}
            </div>

            <DialogFooter>
              <Button variant="outline" onClick={handleClose}>
                {importResult ? 'Close' : 'Cancel'}
              </Button>
              {!importResult && (
                <Button
                  onClick={handleImport}
                  disabled={totalSelectedPhases === 0 || isImporting}
                >
                  {isImporting
                    ? 'Importing...'
                    : `Import ${totalSelectedPhases > 0 ? totalSelectedPhases : ''} Task${totalSelectedPhases !== 1 ? 's' : ''}`}
                </Button>
              )}
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <PreviewModal
          isOpen={previewOpen}
          onClose={() => setPreviewOpen(false)}
          content={previewContent}
          fileName={previewFileName}
          isLoading={previewLoading}
        />
      </>
    );
  }
);

export const ImportPlansDialog = defineModal<ImportPlansDialogProps, void>(
  ImportPlansDialogImpl
);
