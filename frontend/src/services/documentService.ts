import api from './api';
import { Document, DocumentCreate, DocumentType, InvoiceCreate, ReturnInvoiceCreate } from '@/types/document';
import { PaginatedResponse, SearchParams } from '@/types/api';

/** Отримує правильний ендпоінт для типу документа */
function getEndpointForType(type: DocumentType): string {
  switch (type) {
    case 'invoice': return '/invoices';
    case 'return_invoice': return '/return-invoices';
    case 'transfer': return '/transfers';
    case 'write_off': return '/write-offs';
    default: return '/documents';
  }
}

export const documentService = {
  async getDocuments(params?: SearchParams & { document_type?: DocumentType }): Promise<PaginatedResponse<Document>> {
    const response = await api.get<PaginatedResponse<Document>>('/documents', { params });
    return response.data;
  },

  async getDocument(id: string): Promise<Document> {
    const response = await api.get<Document>(`/documents/${id}`);
    return response.data;
  },

  async createDocument(data: DocumentCreate): Promise<Document> {
    // Залежно від типу документа використовуємо відповідний ендпоінт
    switch (data.document_type) {
      case 'invoice': {
        const invoiceData = data as InvoiceCreate;
        const response = await api.post<Document>('/invoices', {
          number: invoiceData.number,
          supplier_id: invoiceData.supplier_id,
          invoice_date: invoiceData.invoice_date,
          payment_method: invoiceData.payment_method || undefined,
          is_fiscal: invoiceData.is_fiscal,
          notes: invoiceData.notes,
          items: invoiceData.items.map(item => ({
            product_id: item.product_id,
            quantity: item.quantity,
            price: item.price,
            total: item.total ?? (item.quantity * item.price),
          })),
        });
        return response.data;
      }
      case 'return_invoice': {
        const returnData = data as ReturnInvoiceCreate;
        const response = await api.post<Document>('/return-invoices', {
          number: returnData.number,
          supplier_id: returnData.supplier_id,
          return_date: returnData.return_date,
          is_fiscal: returnData.is_fiscal,
          notes: returnData.notes,
          items: returnData.items.map(item => ({
            product_id: item.product_id,
            quantity: item.quantity,
            price: item.price,
            total: item.total ?? (item.quantity * item.price),
          })),
        });
        return response.data;
      }
      case 'transfer':
        throw new Error('Transfer creation not implemented yet');
      case 'write_off':
        throw new Error('Write-off creation not implemented yet');
      default:
        throw new Error(`Unknown document type: ${data.document_type}`);
    }
  },

  async updateDocument(id: string, data: Partial<DocumentCreate>): Promise<Document> {
    const response = await api.put<Document>(`/documents/${id}`, data);
    return response.data;
  },

  async confirmDocument(id: string, documentType?: DocumentType): Promise<Document> {
    const endpoint = documentType ? `${getEndpointForType(documentType)}/${id}/confirm` : `/documents/${id}/confirm`;
    // Надсилаємо статус "confirmed" в тілі запиту, як очікує бекенд
    const response = await api.post<Document>(endpoint, { status: 'confirmed' });
    return response.data;
  },

  async cancelDocument(id: string, documentType?: DocumentType): Promise<Document> {
    const endpoint = documentType ? `${getEndpointForType(documentType)}/${id}/cancel` : `/documents/${id}/cancel`;
    // Надсилаємо статус "cancelled" в тілі запиту
    const response = await api.post<Document>(endpoint, { status: 'cancelled' });
    return response.data;
  },

  async deleteDocument(id: string): Promise<void> {
    await api.delete(`/documents/${id}`);
  },
};
