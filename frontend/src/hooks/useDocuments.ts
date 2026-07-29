import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { documentService } from '@/services/documentService';
import { DocumentCreate, DocumentType, InvoiceCreate, ReturnInvoiceCreate } from '@/types/document';
import { SearchParams } from '@/types/api';
import toast from 'react-hot-toast';

type DocumentCreateInput = DocumentCreate | InvoiceCreate | ReturnInvoiceCreate;

/** Отримує текст помилки з response, підтримує Pydantic validation errors */
function getErrorMessage(error: any): string {
  if (error?.response?.data?.detail) {
    const detail = error.response.data.detail;
    // Якщо detail — масив (Pydantic validation errors), формуємо рядок
    if (Array.isArray(detail)) {
      return detail.map((err: any) => {
        const field = err.loc?.slice(1).join('.') || 'field';
        return `${field}: ${err.msg}`;
      }).join('; ');
    }
    // Якщо detail — рядок
    if (typeof detail === 'string') return detail;
    // Якщо detail — об'єкт
    return JSON.stringify(detail);
  }
  return error?.message || 'Невідома помилка';
}

export function useDocuments(params?: SearchParams & {
  document_type?: DocumentType;
  date_from?: string;
  date_to?: string;
  supplier_id?: string;
  amount_from?: string;
  amount_to?: string;
  status?: string;
}) {
  return useQuery({
    queryKey: ['documents', params],
    queryFn: () => documentService.getDocuments(params),
  });
}

export function useDocument(id: string) {
  return useQuery({
    queryKey: ['document', id],
    queryFn: () => documentService.getDocument(id),
    enabled: !!id,
  });
}

export function useCreateDocument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: DocumentCreateInput) => documentService.createDocument(data as any),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['documents'] });
      toast.success('Документ успішно створено');
    },
    onError: (error: any) => {
      toast.error(getErrorMessage(error));
    },
  });
}

export function useConfirmDocument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, documentType }: { id: string; documentType?: DocumentType }) =>
      documentService.confirmDocument(id, documentType),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['documents'] });
      queryClient.invalidateQueries({ queryKey: ['document'] });
      toast.success('Документ підтверджено');
    },
    onError: (error: any) => {
      toast.error(getErrorMessage(error));
    },
  });
}

export function useCancelDocument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, documentType }: { id: string; documentType?: DocumentType }) =>
      documentService.cancelDocument(id, documentType),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['documents'] });
      queryClient.invalidateQueries({ queryKey: ['document'] });
      toast.success('Документ скасовано');
    },
    onError: (error: any) => {
      toast.error(getErrorMessage(error));
    },
  });
}

/** Масове підтвердження документів */
export function useBatchConfirm() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (items: Array<{ id: string; document_type: DocumentType }>) =>
      documentService.batchConfirm(items),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['documents'] });
      toast.success('Документи успішно підтверджено');
    },
    onError: (error: any) => {
      toast.error(getErrorMessage(error));
    },
  });
}

/** Копіювання документа */
export function useCopyDocument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, documentType }: { id: string; documentType: DocumentType }) =>
      documentService.copyDocument(id, documentType),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['documents'] });
      toast.success('Документ скопійовано');
    },
    onError: (error: any) => {
      toast.error(getErrorMessage(error));
    },
  });
}

/** Видалення документа */
export function useDeleteDocument() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, documentType }: { id: string; documentType: DocumentType }) =>
      documentService.deleteDocument(id, documentType),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['documents'] });
      toast.success('Документ видалено');
    },
    onError: (error: any) => {
      toast.error(getErrorMessage(error));
    },
  });
}
