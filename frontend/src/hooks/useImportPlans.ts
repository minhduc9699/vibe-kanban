import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { plansApi, type ImportPlansRequest } from '@/lib/api';
import { taskKeys } from './useTask';

export const planKeys = {
  all: ['plans'] as const,
  list: (projectId: string) => [...planKeys.all, 'list', projectId] as const,
};

export function useImportPlans(projectId: string) {
  const queryClient = useQueryClient();

  const listPlans = useQuery({
    queryKey: planKeys.list(projectId),
    queryFn: () => plansApi.list(projectId),
    enabled: !!projectId,
  });

  const importPlans = useMutation({
    mutationFn: (request: ImportPlansRequest) => plansApi.import(request),
    onSuccess: () => {
      // Invalidate task queries to refresh the task list
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
    },
  });

  return { listPlans, importPlans };
}
