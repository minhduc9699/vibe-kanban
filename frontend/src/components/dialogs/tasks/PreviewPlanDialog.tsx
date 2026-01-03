import { useState, useEffect } from 'react';
import NiceModal, { useModal } from '@ebay/nice-modal-react';
import { defineModal } from '@/lib/modals';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Loader } from '@/components/ui/loader';
import { FileText, AlertCircle } from 'lucide-react';
import { plansApi } from '@/lib/api';
import WYSIWYGEditor from '@/components/ui/wysiwyg';

export interface PreviewPlanDialogProps {
  projectId: string;
  filePath: string;
}

const PreviewPlanDialogImpl = NiceModal.create<PreviewPlanDialogProps>(
  (props) => {
    const { projectId, filePath } = props;
    const modal = useModal();
    const [content, setContent] = useState('');
    const [fileName, setFileName] = useState('Loading...');
    const [isLoading, setIsLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
      async function loadContent() {
        setIsLoading(true);
        setError(null);
        try {
          const result = await plansApi.readFile(projectId, filePath);
          setContent(result.content);
          setFileName(result.file_name);
        } catch (err) {
          setError('Failed to load plan content');
          setFileName('Error');
          console.error('Failed to load plan file:', err);
        } finally {
          setIsLoading(false);
        }
      }
      loadContent();
    }, [projectId, filePath]);

    const handleClose = () => {
      modal.remove();
    };

    return (
      <Dialog open={modal.visible} onOpenChange={handleClose}>
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
            ) : error ? (
              <div className="flex flex-col items-center justify-center py-8 text-destructive">
                <AlertCircle className="h-12 w-12 mb-2" />
                <p>{error}</p>
              </div>
            ) : (
              <WYSIWYGEditor value={content} disabled className="min-h-[200px]" />
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={handleClose}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }
);

export const PreviewPlanDialog = defineModal<PreviewPlanDialogProps, void>(
  PreviewPlanDialogImpl
);
