using System;
using System.Collections.Generic;
using System.Globalization;

namespace Taba
{
    /// <summary>Um campo do <c>PATCH</c> que aceita voltar ao padrão do servidor (<c>null</c>).</summary>
    public struct Field<T> where T : struct, IFormattable
    {
        private readonly int _state;
        private readonly T _value;

        private Field(int state, T value)
        {
            _state = state;
            _value = value;
        }

        /// <summary>Não envia o campo (o valor inicial).</summary>
        public static Field<T> Keep
        {
            get { return default(Field<T>); }
        }

        /// <summary>Envia <c>null</c>: volta ao padrão do servidor.</summary>
        public static Field<T> UseDefault
        {
            get { return new Field<T>(1, default(T)); }
        }

        public static Field<T> Set(T value)
        {
            return new Field<T>(2, value);
        }

        public static implicit operator Field<T>(T value)
        {
            return Set(value);
        }

        internal void Push(string name, List<string> fields)
        {
            if (_state == 1)
            {
                fields.Add("\"" + name + "\":null");
            }
            else if (_state == 2)
            {
                fields.Add("\"" + name + "\":" + _value.ToString(null, CultureInfo.InvariantCulture));
            }
        }
    }
}
