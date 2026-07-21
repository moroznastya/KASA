export interface Supplier {
  id: string;
  name: string;
  edrpou: string | null;
  phone: string | null;
  email: string | null;
  address: string | null;
  notes: string | null;
  created_at: string;
  updated_at: string;
}

export interface SupplierCreate {
  name: string;
  edrpou?: string | null;
  phone?: string | null;
  email?: string | null;
  address?: string | null;
  notes?: string | null;
}

export interface SupplierUpdate {
  name?: string;
  edrpou?: string | null;
  phone?: string | null;
  email?: string | null;
  address?: string | null;
  notes?: string | null;
}
