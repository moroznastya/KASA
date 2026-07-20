import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { documentService } from '@/services/documentService';
import { DocumentCreate, DocumentType } from '@/types/document';
import { SearchParams } from '@/types/api';
import toast from 'react-hot-toast';

export function useDocuments(params?: SearchParams & { document_type?: DocumentType }) {
  return useQuery({
    queryKey: ['documents', params],
    queryFn: () => documentService.getDocuments(params),
  });
}

export function useDocument(id: number) {
  return useQuery({
    queryKey: ['document', id],
    queryFn: () => documentService.getDocument(id),
    enabled: !!id,
  });
}

export function useCreateDocument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: DocumentCreate) => documentService.createDocument(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['documents'] });
      toast.success('Документ успішно створено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка при створенні документа');
    },
  });
}

export function useConfirmDocument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: number) => documentService.confirmDocument(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['documents'] });
      queryClient.invalidateQueries({ queryKey: ['document'] });
      toast.success('Документ підтверджено');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка при підтвердженні документа');
    },
  });
}

export function useCancelDocument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: number) => documentService.cancelDocument(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['documents'] });
      queryClient.invalidateQueries({ queryKey: ['document'] });
      toast.success('Документ скасовано');
    },
    onError: (error: any) => {
      toast.error(error?.response?.data?.detail || 'Помилка при скасуванні документа');
    },
  });
}
