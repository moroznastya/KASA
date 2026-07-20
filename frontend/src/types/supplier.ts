export interface Supplier {
  id: string;
  name: string;
  code: string;
  contact_person: string | null;
  phone: string | null;
  email: string | null;
  address: string | null;
  edrpou: string | null;
  is_active: boolean;
  balance: string;
  created_at: string;
  updated_at: string;
}

export interface SupplierCreate {
  name: string;
  code: string;
  contact_person?: string | null;
  phone?: string | null;
  email?: string | null;
  address?: string | null;
  edrpou?: string | null;
  is_active?: boolean;
}

export interface SupplierUpdate extends SupplierCreate {
  id: string;
}
