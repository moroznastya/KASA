import React from 'react';
import { User, Phone } from 'lucide-react';
import { Debtor } from '@/services/debtorService';
import { formatCurrency } from '@/utils/format';

interface DebtorListItemProps {
  debtor: Debtor;
  onClick: (debtor: Debtor) => void;
}

const DebtorListItem: React.FC<DebtorListItemProps> = ({ debtor, onClick }) => {
  return (
    <div
      onClick={() => onClick(debtor)}
      className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-4 cursor-pointer hover:bg-gray-50 dark:hover:bg-slate-700/50 transition-colors"
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3 min-w-0">
          <div className="w-10 h-10 rounded-full bg-primary-100 dark:bg-primary-900/30 flex items-center justify-center flex-shrink-0">
            <User className="w-5 h-5 text-primary-600 dark:text-primary-400" />
          </div>
          <div className="min-w-0">
            <p className="text-base font-semibold text-gray-900 dark:text-white truncate">
              {debtor.name}
            </p>
            {debtor.phone && (
              <p className="text-sm text-gray-500 dark:text-gray-400 flex items-center gap-1 mt-0.5">
                <Phone className="w-3.5 h-3.5 flex-shrink-0" />
                <span className="truncate">{debtor.phone}</span>
              </p>
            )}
          </div>
        </div>
        <div className="text-right flex-shrink-0 ml-4">
          <p className="text-xl font-bold text-danger-600">
            {formatCurrency(debtor.total_debt)}
          </p>
        </div>
      </div>
    </div>
  );
};

export default DebtorListItem;
