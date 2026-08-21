/** Типи для касових операцій (внесення/інкасація готівки). */

export type CashOperationType = 'deposit' | 'collection';

/** Тип каси: cash — готівкова, card — безготівкова (банківська). */
export type CashType = 'cash' | 'card';

export interface CashOperation {
  id: string;
  store_id: string;
  user_id: string;
  user_name: string;
  operation_type: CashOperationType;
  cash_type: CashType;
  /** Decimal у вигляді рядка (Rust Decimal serde) */
  amount: string;
  comment?: string | null;
  created_at: string;
}

export interface CashOperationsResponse {
  operations: CashOperation[];
  /** Поточні баланси кас (deposit − collection), окремо готівка/безготівка */
  balances: {
    cash: string;
    card: string;
  };
}

export interface CashOperationCreateInput {
  operation_type: CashOperationType;
  cash_type: CashType;
  amount: number;
  comment?: string;
}
