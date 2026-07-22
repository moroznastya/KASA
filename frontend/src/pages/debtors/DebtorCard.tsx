import React from 'react';
import { Link } from 'react-router-dom';
import { Phone, DollarSign, Receipt, CalendarDays } from 'lucide-react';
import { Debtor } from '@/services/debtorService';
import { Receipt as ReceiptType } from '@/types/receipt';
import { formatCurrency } from '@/utils/format';

interface DebtorCardProps {
  debtor: Debtor;
  receipts: ReceiptType[];
  onPay: (debtor: Debtor) => void;
}

const DebtorCard: React.FC<DebtorCardProps> = ({ debtor, receipts, onPay }) => {
  const lastReceipt = receipts.length > 0 ? receipts[receipts.length - 1] : null;

  const formatDate = (dateStr: string) => {
    return new Date(dateStr).toLocaleDateString('uk-UA');
  };

  const getPaidAmount = (receipt: ReceiptType): number => {
    const paid = receipt.paid_amount ? parseFloat(receipt.paid_amount) : 0;
    const total = parseFloat(receipt.total_amount);
    return paid >= total ? total : paid;
  };

  const getDebtAmount = (receipt: ReceiptType): number => {
    const paid = receipt.paid_amount ? parseFloat(receipt.paid_amount) : 0;
    const total = parseFloat(receipt.total_amount);
    return Math.max(0, total - paid);
  };

  return (
    <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-6 space-y-4">
      {/* Debtor Info */}
      <div className="flex items-start justify-between">
        <div className="space-y-1">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
            {debtor.name}
          </h3>
          {debtor.phone && (
            <div className="flex items-center gap-1.5 text-sm text-gray-500 dark:text-gray-400">
              <Phone className="w-4 h-4" />
              <span>{debtor.phone}</span>
            </div>
          )}
        </div>
        <div className="text-right">
          <p className="text-2xl font-bold text-danger-600">
            {formatCurrency(debtor.total_debt)}
          </p>
          {lastReceipt && (
            <div className="flex items-center gap-1 mt-1 text-xs text-gray-400 dark:text-gray-500 justify-end">
              <CalendarDays className="w-3.5 h-3.5" />
              <span>{formatDate(lastReceipt.created_at)}</span>
            </div>
          )}
        </div>
      </div>

      {/* Pay Button */}
      <button
        onClick={() => onPay(debtor)}
        className="w-full inline-flex items-center justify-center gap-2 px-4 py-2 bg-green-600 hover:bg-green-700 text-white text-sm font-medium rounded-lg transition-colors"
      >
        <DollarSign className="w-4 h-4" />
        Сплатити
      </button>

      {/* Receipts Section */}
      <div>
        <div className="flex items-center gap-2 mb-3">
          <Receipt className="w-4 h-4 text-gray-400" />
          <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">
            Чеки боржника
          </h4>
        </div>

        {receipts.length === 0 ? (
          <p className="text-sm text-gray-400 dark:text-gray-500 text-center py-4">
            Немає чеків
          </p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-gray-200 dark:border-slate-700">
                  <th className="text-left py-2 text-gray-500 dark:text-gray-400 font-medium">Номер чеку</th>
                  <th className="text-left py-2 text-gray-500 dark:text-gray-400 font-medium">Дата</th>
                  <th className="text-right py-2 text-gray-500 dark:text-gray-400 font-medium">Сума</th>
                  <th className="text-right py-2 text-gray-500 dark:text-gray-400 font-medium">Сплачено</th>
                  <th className="text-right py-2 text-gray-500 dark:text-gray-400 font-medium">Борг</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-slate-700">
                {receipts.map((receipt) => (
                  <tr key={receipt.id}>
                    <td className="py-2">
                      <Link
                        to={`/receipts/${receipt.id}`}
                        className="text-primary-600 hover:text-primary-700 dark:text-primary-400 dark:hover:text-primary-300 font-medium"
                      >
                        {receipt.receipt_number}
                      </Link>
                    </td>
                    <td className="py-2 text-gray-500 dark:text-gray-400">
                      {formatDate(receipt.created_at)}
                    </td>
                    <td className="py-2 text-right text-gray-900 dark:text-white">
                      {formatCurrency(receipt.total_amount)}
                    </td>
                    <td className="py-2 text-right text-gray-900 dark:text-white">
                      {formatCurrency(getPaidAmount(receipt))}
                    </td>
                    <td className="py-2 text-right text-danger-600 font-medium">
                      {formatCurrency(getDebtAmount(receipt))}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
};

export default DebtorCard;
