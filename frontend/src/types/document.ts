export type DocumentType = 'invoice' | 'transfer' | 'write_off' | 'return_invoice';
export type DocumentStatus = 'draft' | 'confirmed' | 'cancelled';

export interface DocumentItem {
  id: string;
  product_id: string;
  product_name: string;
  product_barcode: string | null;
  quantity: number;
  price: string;
  total: string;
}

export interface Document {
  id: string;
  document_type: DocumentType;
  document_number: string;
  status: DocumentStatus;
  supplier_id: string | null;
  supplier_name?: string;
  from_location: string | null;
  to_location: string | null;
  notes: string | null;
  items: DocumentItem[];
  total_amount: string;
  created_by: string;
  created_by_name?: string;
  created_at: string;
  updated_at: string;
  confirmed_at: string | null;
}

export interface DocumentCreate {
  document_type: DocumentType;
  supplier_id?: string | null;
  from_location?: string | null;
  to_location?: string | null;
  notes?: string | null;
  items: DocumentItemCreate[];
}

export interface DocumentItemCreate {
  product_id: string;
  quantity: number;
  price?: number;
}

export interface InvoiceCreate extends DocumentCreate {
  document_type: 'invoice';
  supplier_id: string;
}

export interface TransferCreate extends DocumentCreate {
  document_type: 'transfer';
  from_location: string;
  to_location: string;
}

export interface WriteOffCreate extends DocumentCreate {
  document_type: 'write_off';
  notes?: string;
}

export interface ReturnInvoiceCreate extends DocumentCreate {
  document_type: 'return_invoice';
  supplier_id: string;
}
