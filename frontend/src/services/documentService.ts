import api from './api';
import { Document, DocumentCreate, DocumentType } from '@/types/document';
import { PaginatedResponse, SearchParams } from '@/types/api';

export const documentService = {
  async getDocuments(params?: SearchParams & { document_type?: DocumentType }): Promise<PaginatedResponse<Document>> {
    const response = await api.get<PaginatedResponse<Document>>('/documents', { params });
    return response.data;
  },

  async getDocument(id: number): Promise<Document> {
    const response = await api.get<Document>(`/documents/${id}`);
    return response.data;
  },

  async createDocument(data: DocumentCreate): Promise<Document> {
    const response = await api.post<Document>('/documents', data);
    return response.data;
  },

  async updateDocument(id: number, data: Partial<DocumentCreate>): Promise<Document> {
    const response = await api.put<Document>(`/documents/${id}`, data);
    return response.data;
  },

  async confirmDocument(id: number): Promise<Document> {
    const response = await api.post<Document>(`/documents/${id}/confirm`);
    return response.data;
  },

  async cancelDocument(id: number): Promise<Document> {
    const response = await api.post<Document>(`/documents/${id}/cancel`);
    return response.data;
  },

  async deleteDocument(id: number): Promise<void> {
    await api.delete(`/documents/${id}`);
  },
};
